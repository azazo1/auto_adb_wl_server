use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    process::Output,
    time::Duration,
};

use serde::Deserialize;
use tokio::{process::Command, sync::oneshot, task::JoinHandle};
use tracing::{info, warn};

const SCRCPY_STARTUP_TIMEOUT: Duration = Duration::from_secs(1);
const SCRCPY_RETRY_INTERVAL: Duration = Duration::from_secs(2);
const SCRCPY_DEVICE_DISCONNECTED_EXIT_CODE: i32 = 2;

pub type ScrcpySuperviseStopTx = oneshot::Sender<()>;

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

    pub fn connection_ip(&self) -> Option<IpAddr> {
        match self {
            Self::TcpIpConnect(addr) => Some(addr.ip()),
            Self::Serial(serial) => connection_ip_from_target(serial),
            Self::Usb | Self::TcpIp => None,
        }
    }
}

pub fn connection_ip_from_target(target: &str) -> Option<IpAddr> {
    target
        .parse::<SocketAddr>()
        .map(|addr| addr.ip())
        .or_else(|_| target.parse::<IpAddr>())
        .ok()
}

pub async fn scrcpy_launch(
    mode: ScrcpyLaunchMode,
) -> Result<Option<(IpAddr, ScrcpySuperviseStopTx)>, String> {
    let (stop_tx, stop_rx) = match mode.connection_ip() {
        Some(ip) => {
            let (stop_tx, stop_rx) = oneshot::channel();
            (Some((ip, stop_tx)), Some(stop_rx))
        }
        None => (None, None),
    };

    let mut handle = spawn_scrcpy(mode.clone(), false)?;
    match tokio::time::timeout(SCRCPY_STARTUP_TIMEOUT, &mut handle).await {
        // 一秒之内退出了, 说明 scrcpy 没有正确启动.
        Ok(scrcpy_result) => {
            let output = join_scrcpy_task(scrcpy_result)?;
            if output.status.success() {
                Ok(None)
            } else {
                Err(format_scrcpy_failure(&output))
            }
        }
        Err(_) => {
            tokio::spawn(async move {
                supervise_scrcpy(mode, stop_rx, handle).await;
            });
            Ok(stop_tx)
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

enum ScrcpyWaitResult {
    Stopped,
    Exited(Output),
}

async fn wait_scrcpy_or_stop(
    stop_rx: Option<&mut oneshot::Receiver<()>>,
    handle: &mut JoinHandle<Result<Output, String>>,
) -> Result<ScrcpyWaitResult, String> {
    if let Some(stop_rx) = stop_rx {
        tokio::select! {
            _ = stop_rx => Ok(ScrcpyWaitResult::Stopped),
            scrcpy_result = handle => join_scrcpy_task(scrcpy_result).map(ScrcpyWaitResult::Exited),
        }
    } else {
        join_scrcpy_task(handle.await).map(ScrcpyWaitResult::Exited)
    }
}

enum ScrcpyStartupResult {
    Stopped,
    Started,
    Exited(Output),
}

async fn wait_scrcpy_startup_or_stop(
    stop_rx: Option<&mut oneshot::Receiver<()>>,
    handle: &mut JoinHandle<Result<Output, String>>,
) -> Result<ScrcpyStartupResult, String> {
    if let Some(stop_rx) = stop_rx {
        tokio::select! {
            _ = stop_rx => Ok(ScrcpyStartupResult::Stopped),
            startup_result = tokio::time::timeout(SCRCPY_STARTUP_TIMEOUT, handle) => {
                match startup_result {
                    Err(_) => Ok(ScrcpyStartupResult::Started),
                    Ok(scrcpy_result) => join_scrcpy_task(scrcpy_result).map(ScrcpyStartupResult::Exited),
                }
            }
        }
    } else {
        match tokio::time::timeout(SCRCPY_STARTUP_TIMEOUT, handle).await {
            Err(_) => Ok(ScrcpyStartupResult::Started),
            Ok(scrcpy_result) => join_scrcpy_task(scrcpy_result).map(ScrcpyStartupResult::Exited),
        }
    }
}

async fn wait_retry_interval_or_stop(stop_rx: Option<&mut oneshot::Receiver<()>>) -> bool {
    if let Some(stop_rx) = stop_rx {
        tokio::select! {
            _ = stop_rx => false,
            _ = tokio::time::sleep(SCRCPY_RETRY_INTERVAL) => true,
        }
    } else {
        tokio::time::sleep(SCRCPY_RETRY_INTERVAL).await;
        true
    }
}

async fn supervise_scrcpy(
    mode: ScrcpyLaunchMode,
    mut stop_rx: Option<oneshot::Receiver<()>>,
    mut handle: JoinHandle<Result<Output, String>>,
) {
    loop {
        let output = match wait_scrcpy_or_stop(stop_rx.as_mut(), &mut handle).await {
            Ok(ScrcpyWaitResult::Stopped) => {
                info!(?mode, "scrcpy supervise canceled while waiting for exit");
                return;
            }
            Ok(ScrcpyWaitResult::Exited(output)) => output,
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
            if !wait_retry_interval_or_stop(stop_rx.as_mut()).await {
                info!(?mode, "scrcpy supervise canceled before reconnect");
                return;
            }
            info!(?mode, "retrying scrcpy reconnect");
            handle = match spawn_scrcpy(mode.clone(), true) {
                Ok(handle) => handle,
                Err(e) => {
                    warn!(e, ?mode, "failed to prepare scrcpy reconnect");
                    continue;
                }
            };

            match wait_scrcpy_startup_or_stop(stop_rx.as_mut(), &mut handle).await {
                Ok(ScrcpyStartupResult::Stopped) => {
                    info!(?mode, "scrcpy supervise canceled during reconnect");
                    return;
                }
                Ok(ScrcpyStartupResult::Started) => {
                    info!(?mode, "scrcpy reconnected");
                    break;
                }
                Ok(ScrcpyStartupResult::Exited(output)) => {
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
                Err(e) => {
                    warn!(e, ?mode, "failed to relaunch scrcpy");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

    use super::{ScrcpyLaunchMode, connection_ip_from_target};

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

    #[test]
    fn tcpip_connect_mode_exposes_connection_ip() {
        let mode = ScrcpyLaunchMode::TcpIpConnect(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(192, 168, 1, 10),
            5555,
        )));

        assert_eq!(
            mode.connection_ip(),
            Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)))
        );
    }

    #[test]
    fn target_parser_extracts_ip_from_socket_addr() {
        assert_eq!(
            connection_ip_from_target("192.168.1.10:5555"),
            Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)))
        );
    }

    #[test]
    fn serial_mode_uses_ip_target_when_available() {
        let mode = ScrcpyLaunchMode::Serial("192.168.1.20:5555".to_string());

        assert_eq!(
            mode.connection_ip(),
            Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)))
        );
    }
}
