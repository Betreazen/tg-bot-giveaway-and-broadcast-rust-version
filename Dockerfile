# syntax=docker/dockerfile:1
# The build stage doubles as the Linux build for bare-metal deployment:
#   docker build --target build -t tg-giveaway:build .
FROM rust:1.98.1-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml messages.json ./
COPY src/ src/
COPY migrations/ migrations/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release && cp target/release/tg-bot-giveaway-and-broadcast /usr/local/bin/bot

FROM build AS test
COPY tests/ tests/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo test --locked

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=build /usr/local/bin/bot /app/bot
ENV DATA_DIR=/app/data
USER 65532:65532
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 CMD ["/app/bot", "--healthcheck"]
ENTRYPOINT ["/app/bot"]
