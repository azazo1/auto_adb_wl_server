use std::{net::SocketAddr, path::PathBuf, process::Output, time::Duration};

use serde::Deserialize;
use tokio::{process::Command, task::JoinHandle};
use tracing::{info, warn};

const SCRCPY_STARTUP_TIMEOUT: Duration = Duration::from_secs(1);
const SCRCPY_RETRY_INTERVAL: Duration = Duration::from_secs(2);
const SCRCPY_DEVICE_DISCONNECTED_EXIT_CODE: i32 = 2;

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "tag", content = "content")]
pub enum ScrcpyLaunchMode {
    Usb,
    TcpIp,
    Serial(String),
    TcpIpConnect(SocketAddr),
}

impl ScrcpyLaunchMode {
    fn command_args(&self, force_reconnect: bool) -> Vec<String> {
        match self {
            Self::Usb => vec!["-d".to_string()],
            Self::TcpIp => vec!["-e".to_string()],
            Self::Serial(serial) => vec!["-s".to_string(), serial.clone()],
            Self::TcpIpConnect(addr) => {
                let reconnect_prefix = if force_reconnect { "+" } else { "" };
                vec![format!("--tcpip={reconnect_prefix}{addr}")]
            }
        }
    }
}

pub async fn scrcpy_launch(mode: ScrcpyLaunchMode) -> Result<(), String> {
    let mut handle = spawn_scrcpy(mode.clone(), false)?;
    match tokio::time::timeout(SCRCPY_STARTUP_TIMEOUT, &mut handle).await {
        // 一秒之内退出了, 说明 scrcpy 没有正确启动.
        Ok(scrcpy_result) => {
            let output = join_scrcpy_task(scrcpy_result)?;
            if output.status.success() {
                Ok(())
            } else {
                Err(format_scrcpy_failure(&output))
            }
        }
        Err(_) => {
            tokio::spawn(async move {
                supervise_scrcpy(mode, handle).await;
            });
            Ok(())
        }
    }
}

fn spawn_scrcpy(
    mode: ScrcpyLaunchMode,
    force_reconnect: bool,
) -> Result<JoinHandle<Result<Output, String>>, String> {
    let scrcpy = which::which("scrcpy").map_err(|_| "scrcpy not found")?;
    Ok(tokio::spawn(run_scrcpy(scrcpy, mode, force_reconnect)))
}

async fn run_scrcpy(
    scrcpy: PathBuf,
    mode: ScrcpyLaunchMode,
    force_reconnect: bool,
) -> Result<Output, String> {
    let mut cmd = Command::new(scrcpy);
    cmd.args(mode.command_args(force_reconnect));
    cmd.output().await.map_err(|e| e.to_string())
}

fn join_scrcpy_task(
    scrcpy_result: Result<Result<Output, String>, tokio::task::JoinError>,
) -> Result<Output, String> {
    scrcpy_result
        .map_err(|e| format!("scrcpy task failed: {e}"))?
        .map_err(|e| format!("failed to launch scrcpy: {e}"))
}

fn format_scrcpy_failure(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("scrcpy returned error code: {:?}", output.status.code())
    } else {
        format!(
            "scrcpy returned error code: {:?}\n{stderr}",
            output.status.code()
        )
    }
}

async fn supervise_scrcpy(mode: ScrcpyLaunchMode, mut handle: JoinHandle<Result<Output, String>>) {
    loop {
        let output = match join_scrcpy_task(handle.await) {
            Ok(output) => output,
            Err(e) => {
                warn!(e, ?mode, "scrcpy process failed");
                return;
            }
        };

        if output.status.success() {
            info!(?mode, "scrcpy exited normally");
            return;
        }

        if output.status.code() != Some(SCRCPY_DEVICE_DISCONNECTED_EXIT_CODE) {
            warn!(
                error = format_scrcpy_failure(&output),
                ?mode,
                "scrcpy exited unexpectedly"
            );
            return;
        }

        warn!(?mode, "scrcpy disconnected, starting reconnect loop");
        loop {
            tokio::time::sleep(SCRCPY_RETRY_INTERVAL).await;
            info!(?mode, "retrying scrcpy reconnect");
            handle = match spawn_scrcpy(mode.clone(), true) {
                Ok(handle) => handle,
                Err(e) => {
                    warn!(e, ?mode, "failed to prepare scrcpy reconnect");
                    continue;
                }
            };

            match tokio::time::timeout(SCRCPY_STARTUP_TIMEOUT, &mut handle).await {
                Err(_) => {
                    info!(?mode, "scrcpy reconnected");
                    break;
                }
                Ok(scrcpy_result) => {
                    let output = match join_scrcpy_task(scrcpy_result) {
                        Ok(output) => output,
                        Err(e) => {
                            warn!(e, ?mode, "failed to relaunch scrcpy");
                            continue;
                        }
                    };

                    if output.status.success() {
                        info!(?mode, "scrcpy reconnect attempt exited normally");
                        return;
                    }

                    warn!(
                        error = format_scrcpy_failure(&output),
                        ?mode,
                        "scrcpy reconnect attempt failed"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use super::ScrcpyLaunchMode;

    #[test]
    fn tcpip_connect_retry_uses_forced_reconnect_arg() {
        let mode = ScrcpyLaunchMode::TcpIpConnect(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(192, 168, 1, 10),
            5555,
        )));

        assert_eq!(
            mode.command_args(true),
            vec!["--tcpip=+192.168.1.10:5555".to_string()]
        );
    }

    #[test]
    fn serial_mode_keeps_serial_arg() {
        let mode = ScrcpyLaunchMode::Serial("device-serial".to_string());

        assert_eq!(
            mode.command_args(false),
            vec!["-s".to_string(), "device-serial".to_string()]
        );
    }
}
