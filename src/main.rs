use anyhow::{Context, Result, bail};
use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};
use teloxide::{prelude::*, types::AllowedUpdate, update_listeners::Polling};
use tg_bot_giveaway_and_broadcast::{
    config::{Config, data_dir_from_env},
    db::Database,
    handlers::{App, handle_callback, handle_message},
    health, import,
    mailing::mailing_worker,
    network::{POLLING_TIMEOUT, redact},
};

const USAGE: &str = "supported options: --check, --healthcheck, --version, import <dir>";
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(30);

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let token = std::env::var("BOT_TOKEN").unwrap_or_default();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", redact(&format!("{error:#}"), token.trim()));
            ExitCode::FAILURE
        }
    }
}

fn init_logging() {
    // RUST_LOG wins; otherwise the Python bot's LOG_LEVEL (INFO, DEBUG, ...).
    let level = std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into());
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new(level.trim().to_lowercase()))
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

async fn run() -> Result<()> {
    init_logging();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--version"] => {
            println!(
                "tg-bot-giveaway-and-broadcast {}",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        ["--healthcheck"] => health::healthcheck(&data_dir_from_env()).await,
        ["import", dir] => import_dump(PathBuf::from(dir)).await,
        ["--check"] => check().await,
        [] => serve().await,
        _ => bail!(USAGE),
    }
}

async fn import_dump(dir: PathBuf) -> Result<()> {
    let db = Database::open(&data_dir_from_env()).await?;
    let report = import::run(&db, &dir).await;
    db.close().await;
    println!("{}", report?);
    Ok(())
}

/// Configuration, migrations and an integrity check, without contacting Telegram.
async fn check() -> Result<()> {
    let app = App::open(Config::from_env()?).await?;
    app.db.check().await?;
    app.db.close().await;
    println!("configuration, migrations and database: ok");
    Ok(())
}

async fn serve() -> Result<()> {
    let config = Config::from_env()?;
    let bot = Bot::new(&config.token);
    let app = App::open(config).await?;
    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handle_message))
        .branch(Update::filter_callback_query().endpoint(handle_callback));
    let errors_app = app.clone();
    let mut dispatcher = Dispatcher::builder(bot.clone(), handler)
        .dependencies(dptree::deps![app.clone()])
        .error_handler(Arc::new(move |error: anyhow::Error| {
            let app = errors_app.clone();
            async move { tracing::error!(error = %app.redact(&error), "update failed") }
        }))
        .build();
    let listener = Polling::builder(bot.clone())
        .timeout(POLLING_TIMEOUT)
        .allowed_updates(vec![AllowedUpdate::Message, AllowedUpdate::CallbackQuery])
        .build();
    let (db, config, notify) = (app.db.clone(), app.config.clone(), app.mailings.clone());
    let mailings = tokio::spawn(mailing_worker(bot.clone(), db, config, notify));
    let heartbeat = tokio::spawn({
        let dir = app.config.data_dir.clone();
        async move { health::heartbeat(&dir).await }
    });
    let shutdown = dispatcher.shutdown_token();
    // The error text of a failed poll contains the request URL with the token.
    let polling_errors = Arc::new(|_error: teloxide::RequestError| async {
        tracing::warn!("Telegram polling failed; retrying with backoff");
    });
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "bot starting");
    let polling = dispatcher.try_dispatch_with_listener(listener, polling_errors);
    tokio::pin!(polling);
    let result = tokio::select! {
        result = &mut polling => result.context("dispatcher stopped"),
        () = stop_signal() => {
            // An interrupted mailing resumes after restart without duplicates.
            mailings.abort();
            let _drain = shutdown.shutdown();
            tokio::time::timeout(SHUTDOWN_DRAIN, &mut polling)
                .await
                .context("shutdown drain timeout")?
                .context("dispatcher stopped")
        }
    };
    mailings.abort();
    heartbeat.abort();
    app.db.close().await;
    tracing::info!("bot stopped");
    result
}

async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
