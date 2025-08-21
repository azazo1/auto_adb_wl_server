use std::{
    ffi::OsStr,
    net::{SocketAddr, SocketAddrV4},
    path::PathBuf,
};

use lazy_static::lazy_static;
use regex::Regex;
use tokio::{
    self,
    io::{self, AsyncBufReadExt, BufReader},
    net::{TcpListener, TcpStream},
    process::Command,
};
use tracing::{info, warn};

const PORT: u16 = 15555;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let listen_addr = format!("0.0.0.0:{PORT}");
    let server = TcpListener::bind(&listen_addr).await.unwrap();
    info!("Listening on {listen_addr}");
    loop {
        let Ok((client, client_addr)) = server.accept().await else {
            warn!("Server accept failed.");
            continue;
        };
        info!("Accept client from {client_addr}");
        tokio::spawn(async move {
            match handle_client(client, client_addr).await {
                Ok(()) => {
                    info!("Client {client_addr} quitted");
                }
                Err(e) => {
                    warn!("Client {client_addr} quitted with error {e:?}");
                }
            }
        });
    }
}

async fn call_adb(args: &[impl AsRef<OsStr>]) -> io::Result<()> {
    lazy_static! {
        static ref ADB: PathBuf = which::which("adb").unwrap();
    }
    let child = Command::new(ADB.as_os_str()).args(args).spawn()?;
    let output = child.wait_with_output().await?;
    info!("adb: {}", String::from_utf8_lossy(&output.stdout));
    info!("adb_err: {}", String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("exit: {}", output.status.code().unwrap_or(-1)),
        ))
    }
}

async fn adb_connect(addr: &str) -> io::Result<()> {
    call_adb(&["connect", addr]).await
}

async fn handle_client(mut client: TcpStream, client_addr: SocketAddr) -> io::Result<()> {
    lazy_static! {
        static ref PAT_CONNECT: Regex = Regex::new(r#"^c-(\d+.\d+.\d+.\d+:\d+)$"#).unwrap();
    }
    let (r, _w) = client.split();
    let mut lines = BufReader::new(r).lines();
    while let Some(line) = lines.next_line().await? {
        info!("{client_addr} received: {line}");
        if let Some(cap) = PAT_CONNECT.captures(&line) {
            if let Some(adb_connect_addr) = cap.get(1) {
                let adb_connect_addr = adb_connect_addr.as_str();
                info!("Adb connect addr: {adb_connect_addr}");
                adb_connect(adb_connect_addr)
                    .await
                    .inspect_err(|e| warn!("adb: {e:?}"))
                    .ok();
            }
        }
    }
    Ok(())
}
