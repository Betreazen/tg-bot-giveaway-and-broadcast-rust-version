//! Google Sheets sync: the Python bot's five sheets, fully rewritten on every run.
//! Plain REST: a service-account JWT (RS256) is exchanged for an access token, then each
//! sheet is created or grown, cleared and written with a single RAW update.
mod rows;

use crate::{config::Config, db::Database, network::SHEETS_TIMEOUT, time};
use anyhow::{Context, Result, anyhow};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use reqwest::{Client, RequestBuilder, Response, Url};
use ring::{
    rand::SystemRandom,
    signature::{RSA_PKCS1_SHA256, RsaKeyPair},
};
use rows::collect;
use serde::Deserialize;
use serde_json::{Value, json};

const SHEETS_API: &str = "https://sheets.googleapis.com";
const SCOPE: &str =
    "https://www.googleapis.com/auth/spreadsheets https://www.googleapis.com/auth/drive";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";
const TOKEN_LIFETIME_SECS: i64 = 3600;
/// Python grows the grid to the number of written rows plus this margin.
const SPARE_ROWS: usize = 10;

type Rows = Vec<Vec<Value>>;

pub enum SyncOutcome {
    Done,
    Skipped,
}

// Deliberately no Debug: holds the private key.
#[derive(Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    pub private_key: String,
    pub private_key_id: String,
    pub token_uri: String,
}

/// Skipped when sync is disabled or not configured, as in Python; otherwise a full sync.
pub async fn sync_all(db: &Database, config: &Config) -> Result<SyncOutcome> {
    if !config.sheets_enabled {
        tracing::info!("Google Sheets sync is disabled (SHEETS_SYNC_ENABLED=false)");
        return Ok(SyncOutcome::Skipped);
    }
    let (Some(path), Some(spreadsheet_id)) =
        (&config.google_credentials_path, &config.spreadsheet_id)
    else {
        tracing::warn!("Google Sheets credentials path or spreadsheet id is not configured");
        return Ok(SyncOutcome::Skipped);
    };
    let raw = tokio::fs::read(path)
        .await
        .with_context(|| format!("cannot read {}", path.display()))?;
    let account: ServiceAccount =
        serde_json::from_slice(&raw).context("invalid Google service account file")?;
    sync_with(db, &account, spreadsheet_id, SHEETS_API).await?;
    Ok(SyncOutcome::Done)
}

pub async fn sync_with(
    db: &Database,
    account: &ServiceAccount,
    spreadsheet_id: &str,
    sheets_api: &str,
) -> Result<()> {
    let sheets = collect(db).await?;
    let client = Client::builder()
        .timeout(SHEETS_TIMEOUT)
        .build()
        .context("cannot build HTTP client")?;
    let token = access_token(&client, account).await?;
    let api = Api {
        client,
        token,
        root: Url::parse(sheets_api).context("invalid Sheets API URL")?,
        id: spreadsheet_id.to_owned(),
    };
    let existing = api.sheets().await?;
    for (title, values) in &sheets {
        api.rewrite(&existing, title, values).await?;
        tracing::info!(
            sheet = title,
            rows = values.len() - 1,
            "Google Sheets sheet updated"
        );
    }
    Ok(())
}

/// Signed service-account assertion for Google's OAuth token endpoint.
pub fn jwt(account: &ServiceAccount, now_unix: i64) -> Result<String> {
    let header = json!({"alg": "RS256", "typ": "JWT", "kid": account.private_key_id});
    let claims = json!({
        "iss": account.client_email,
        "scope": SCOPE,
        "aud": account.token_uri,
        "iat": now_unix,
        "exp": now_unix + TOKEN_LIFETIME_SECS,
    });
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let key = RsaKeyPair::from_pkcs8(&pem_to_der(&account.private_key)?)
        .map_err(|e| anyhow!("invalid service account private key: {e}"))?;
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .map_err(|_| anyhow!("cannot sign the Google OAuth assertion"))?;
    Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let body: String = pem
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("-----"))
        .collect();
    // The decode error would quote a key character, so it is not passed on.
    STANDARD
        .decode(body)
        .map_err(|_| anyhow!("service account private key is not valid PEM"))
}

async fn access_token(client: &Client, account: &ServiceAccount) -> Result<String> {
    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }
    let assertion = jwt(account, time::now().timestamp())?;
    let request = client
        .post(&account.token_uri)
        .form(&[("grant_type", GRANT_TYPE), ("assertion", &assertion)]);
    let response = send(request, "OAuth token").await?;
    let body: TokenResponse = response
        .json()
        .await
        .context("Google OAuth token: invalid response")?;
    Ok(body.access_token)
}

/// Error texts carry the step and HTTP status, never the token or the request body.
async fn send(request: RequestBuilder, step: &str) -> Result<Response> {
    let response = request
        .send()
        .await
        .with_context(|| format!("Google {step}: request failed"))?;
    let status = response.status();
    response
        .error_for_status()
        .with_context(|| format!("Google {step}: HTTP {status}"))
}

#[derive(Deserialize)]
struct Spreadsheet {
    #[serde(default)]
    sheets: Vec<Sheet>,
}

#[derive(Deserialize)]
struct Sheet {
    properties: SheetProperties,
}

// Google omits zero values in JSON (the first sheet's id is often 0).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SheetProperties {
    #[serde(default)]
    sheet_id: i64,
    title: String,
    #[serde(default)]
    grid_properties: GridProperties,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GridProperties {
    #[serde(default)]
    row_count: usize,
    #[serde(default)]
    column_count: usize,
}

struct Api {
    client: Client,
    token: String,
    root: Url,
    id: String,
}

impl Api {
    /// `root/v4/spreadsheets/<tail…>`, each tail item one percent-encoded path segment.
    fn url(&self, tail: &[&str]) -> Result<Url> {
        let mut url = self.root.clone();
        url.path_segments_mut()
            .map_err(|()| anyhow!("invalid Sheets API URL"))?
            .pop_if_empty()
            .extend(["v4", "spreadsheets"])
            .extend(tail);
        Ok(url)
    }

    async fn call(&self, request: RequestBuilder, step: &str) -> Result<Response> {
        send(request.bearer_auth(&self.token), &format!("Sheets {step}")).await
    }

    async fn sheets(&self) -> Result<Vec<SheetProperties>> {
        let request = self
            .client
            .get(self.url(&[&self.id])?)
            .query(&[("fields", "sheets.properties")]);
        let spreadsheet: Spreadsheet = self
            .call(request, "metadata")
            .await?
            .json()
            .await
            .context("Google Sheets metadata: invalid response")?;
        Ok(spreadsheet
            .sheets
            .into_iter()
            .map(|s| s.properties)
            .collect())
    }

    /// Python's `_rewrite`: make the grid fit, clear the sheet, write everything at A1.
    async fn rewrite(
        &self,
        existing: &[SheetProperties],
        title: &str,
        values: &Rows,
    ) -> Result<()> {
        let rows = values.len() + SPARE_ROWS;
        let cols = values.first().map_or(1, |header| header.len().max(1));
        let current = existing.iter().find(|sheet| sheet.title == title);
        if let Some(change) = grid_change(current, title, rows, cols) {
            let url = self.url(&[&format!("{}:batchUpdate", self.id)])?;
            let body = json!({"requests": [change]});
            let request = self.client.post(url).json(&body);
            self.call(request, &format!("grid of {title}")).await?;
        }
        let url = self.url(&[&self.id, "values", &format!("'{title}':clear")])?;
        let request = self.client.post(url).json(&json!({}));
        self.call(request, &format!("clear of {title}")).await?;
        let url = self.url(&[&self.id, "values", &format!("'{title}'!A1")])?;
        let request = self
            .client
            .put(url)
            .query(&[("valueInputOption", "RAW")])
            .json(&json!({"values": values}));
        self.call(request, &format!("update of {title}")).await?;
        Ok(())
    }
}

/// `addSheet` for a missing sheet, `updateSheetProperties` for one that is too small.
fn grid_change(
    current: Option<&SheetProperties>,
    title: &str,
    rows: usize,
    cols: usize,
) -> Option<Value> {
    let Some(sheet) = current else {
        return Some(json!({"addSheet": {"properties": {
            "title": title,
            "gridProperties": {"rowCount": rows, "columnCount": cols},
        }}}));
    };
    let grid = &sheet.grid_properties;
    if grid.row_count >= rows && grid.column_count >= cols {
        return None;
    }
    Some(json!({"updateSheetProperties": {
        "properties": {
            "sheetId": sheet.sheet_id,
            "gridProperties": {
                "rowCount": rows.max(grid.row_count),
                "columnCount": cols.max(grid.column_count),
            },
        },
        "fields": "gridProperties.rowCount,gridProperties.columnCount",
    }}))
}
