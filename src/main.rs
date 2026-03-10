use std::net::SocketAddr;

use auto_adb_wl_server::{
    adb::{adb_connect, adb_pair},
    mdns::MDnsService,
    scrcpy::{ScrcpyLaunchMode, scrcpy_launch},
};
use axum::{Json, Router, http::StatusCode, routing::post};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::{error, info, warn};

#[derive(clap::Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct AppArgs {
    #[clap(short, long, default_value_t = 21300)]
    port: u16,
}

#[derive(Deserialize)]
struct AdbConnectArgs {
    address: SocketAddr,
}

#[derive(Deserialize)]
struct AdbPairArgs {
    address: SocketAddr,
    pair_code: String,
}

#[derive(Deserialize)]
struct ScrcpyLaunchArgs {
    mode: ScrcpyLaunchMode,
}

#[derive(Serialize)]
struct ApiResponse {
    message: String,
    ok: bool,
}

impl ApiResponse {
    fn new(ok: bool, message: impl Into<String>) -> Self {
        Self {
            ok,
            message: message.into(),
        }
    }
}

async fn handler_adb_pair(
    Json(AdbPairArgs { address, pair_code }): Json<AdbPairArgs>,
) -> (StatusCode, Json<ApiResponse>) {
    info!("req: adb pair {address:?} {pair_code}");
    if let Err(e) = adb_pair(address, pair_code).await {
        warn!(e, "adb pair failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::new(false, e)),
        );
    }
    (StatusCode::OK, Json(ApiResponse::new(true, "paired")))
}

async fn handler_adb_connect(
    Json(AdbConnectArgs { address }): Json<AdbConnectArgs>,
) -> (StatusCode, Json<ApiResponse>) {
    info!("req: adb connect {address:?}");
    if let Err(e) = adb_connect(address).await {
        warn!(e, "adb connect failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::new(false, e)),
        );
    }
    (StatusCode::OK, Json(ApiResponse::new(true, "connected")))
}

async fn handler_scrcpy_launch(
    Json(ScrcpyLaunchArgs { mode }): Json<ScrcpyLaunchArgs>,
) -> (StatusCode, Json<ApiResponse>) {
    info!("req: scrcpy launch ({mode:?})");
    if let Err(e) = scrcpy_launch(mode).await {
        warn!(e, "scrcpy launch failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::new(false, e)),
        );
    }
    (StatusCode::OK, Json(ApiResponse::new(true, "launched")))
}

async fn shutdown_signal() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        error!(e = format!("{e:?}"), "failed to receive ctrl-c signal.");
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let args = AppArgs::parse();
    let listener = TcpListener::bind(format!("0.0.0.0:{}", args.port)).await?;
    let mdns = MDnsService::register(args.port).map_err(|e| anyhow::anyhow!("{e}"))?;
    info!("bind on port: {}", args.port);
    let app = Router::new()
        .route("/adb/connect", post(handler_adb_connect))
        .route("/adb/pair", post(handler_adb_pair))
        .route("/scrcpy/launch", post(handler_scrcpy_launch));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    mdns.unregister().map_err(|e| anyhow::anyhow!("{e}"))?;
    info!("shutdown");
    Ok(())
}
