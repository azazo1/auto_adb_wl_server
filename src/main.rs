use std::{ffi::OsStr, net::SocketAddr, path::PathBuf, process::Output};

use lazy_static::lazy_static;
use regex::Regex;
use tokio::{
    self,
    io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
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

async fn call_adb(args: &[impl AsRef<OsStr>]) -> io::Result<Output> {
    lazy_static! {
        static ref ADB: PathBuf = which::which("adb").unwrap();
    }
    let output = Command::new(ADB.as_os_str()).args(args).output().await?;
    info!("adb: {}", String::from_utf8_lossy(&output.stdout).trim());
    let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !error.is_empty() {
        info!("adb_err: {}", error);
    }
    if output.status.success() {
        Ok(output)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("exit: {}", output.status.code().unwrap_or(-1)),
        ))
    }
}

async fn adb_connect(addr: &str) -> io::Result<()> {
    call_adb(&["connect", addr]).await.map(|_| ())
}

async fn adb_disconnect(addr: &str) -> io::Result<()> {
    call_adb(&["disconnect", addr]).await.map(|_| ())
}

async fn adb_pair(addr: &str, code: &str) -> io::Result<bool> {
    call_adb(&["pair", addr, code])
        .await
        .map(|x| x.status.success())
}

async fn handle_client(mut client: TcpStream, client_addr: SocketAddr) -> io::Result<()> {
    lazy_static! {
        static ref PAT_CONNECT: Regex = Regex::new(r#"^c-(\d+.\d+.\d+.\d+:\d+)$"#).unwrap();
        static ref PAT_DISCONNECT: Regex = Regex::new(r#"^d-(.+)$"#).unwrap();
        static ref PAT_PAIR: Regex = Regex::new(r#"^p-(\d+.\d+.\d+.\d+:\d+)-(\d{6})$"#).unwrap();
    }
    let (r, w) = client.split();
    let mut writer = BufWriter::new(w);
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
        } else if let Some(cap) = PAT_DISCONNECT.captures(&line) {
            if let Some(target) = cap.get(1) {
                let target = target.as_str();
                info!("Adb disconnect target: {target}");
                adb_disconnect(target)
                    .await
                    .inspect_err(|e| warn!("adb: {e:?}"))
                    .ok();
            }
        } else if let Some(cap) = PAT_PAIR.captures(&line) {
            let Some(address) = cap.get(1) else {
                continue;
            };
            let Some(code) = cap.get(2) else {
                continue;
            };
            let address = address.as_str();
            let code = code.as_str();
            info!("Adb pair address: {address}, code: {code}");
            let suc = adb_pair(address, code)
                .await
                .inspect_err(|e| warn!("adb: {e:?}"))
                .unwrap_or(false);
            if suc {
                writer.write_all("ok\n".as_bytes()).await?;
                writer.flush().await?;
                info!("Adb successfully paired.");
            } else {
                info!("Adb pair failed.");
            }
        }
    }
    Ok(())
}
