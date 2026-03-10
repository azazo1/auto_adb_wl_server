use std::net::SocketAddr;

use auto_adb_wl_server::{
    adb::{adb_connect, adb_pair},
    scrcpy::{ScrcpyLaunchMode, scrcpy_launch},
};
use axum::{Json, Router, http::StatusCode, routing::post};
use clap::Parser;
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing::{info, warn};

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
    pair_code: i32,
}

#[derive(Deserialize)]
struct ScrcpyLaunchArgs {
    mode: ScrcpyLaunchMode,
}

async fn handler_adb_pair(
    Json(AdbPairArgs { address, pair_code }): Json<AdbPairArgs>,
) -> (StatusCode, String) {
    info!("req: adb pair {address:?} {pair_code}");
    if let Err(e) = adb_pair(address, pair_code).await {
        warn!(e, "adb pair failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    (StatusCode::OK, "paired".into())
}

async fn handler_adb_connect(
    Json(AdbConnectArgs { address }): Json<AdbConnectArgs>,
) -> (StatusCode, String) {
    info!("req: adb connect {address:?}");
    if let Err(e) = adb_connect(address).await {
        warn!(e, "adb connect failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    (StatusCode::OK, "connected".into())
}

async fn handler_scrcpy_launch(
    Json(ScrcpyLaunchArgs { mode }): Json<ScrcpyLaunchArgs>,
) -> (StatusCode, String) {
    info!("req: scrcpy launch ({mode:?})");
    if let Err(e) = scrcpy_launch(mode).await {
        warn!(e, "scrcpy launch failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    (StatusCode::OK, "launched".into())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let args = AppArgs::parse();
    let listener = TcpListener::bind(format!("0.0.0.0:{}", args.port)).await?;
    info!("bind on port: {}", args.port);
    let app = Router::new()
        .route("/adb/connect", post(handler_adb_connect))
        .route("/adb/pair", post(handler_adb_pair))
        .route("/scrcpy/launch", post(handler_scrcpy_launch));
    axum::serve(listener, app).await?;
    Ok(())
}
