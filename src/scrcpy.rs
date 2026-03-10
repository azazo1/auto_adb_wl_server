use std::net::SocketAddr;

use serde::Deserialize;
use tokio::process::Command;

#[derive(Deserialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ScrcpyLaunchMode {
    Usb,
    TcpIp,
    Serial(String),
    TcpIpConnect(SocketAddr),
}

pub async fn scrcpy_launch(mode: ScrcpyLaunchMode) -> Result<(), String> {
    let scrcpy = which::which("scrcpy").map_err(|_| "scrcpy not found")?;
    let mut cmd = Command::new(scrcpy);
    match mode {
        ScrcpyLaunchMode::Usb => {
            cmd.arg("-d");
        }
        ScrcpyLaunchMode::TcpIp => {
            cmd.arg("-e");
        }
        ScrcpyLaunchMode::Serial(s) => {
            cmd.arg("-s").arg(s);
        }
        ScrcpyLaunchMode::TcpIpConnect(addr) => {
            cmd.arg("--tcpip").arg(addr.to_string());
        }
    }
    let output = cmd.output().await.map_err(|_| "failed to launch scrcpy")?;
    if !output.status.success() {
        Err(format!(
            "scrcpy returned error code: {:?}\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ))?;
    }
    Ok(())
}
