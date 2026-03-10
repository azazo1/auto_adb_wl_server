use std::{ffi::OsStr, net::SocketAddr};

use tokio::process::Command;

/// 使用指定的参数调用 adb
///
/// # Returns
/// 返回 adb 的标准输出
///
/// # Errors
/// 当 adb 无法启动或者操作执行失败, 返回大致的错误信息.
async fn call_adb(args: &[impl AsRef<OsStr>]) -> Result<String, String> {
    let adb = which::which("adb").map_err(|_| "adb not found")?;
    let mut cmd = Command::new(&adb);
    cmd.args(args);
    let output = cmd.output().await.map_err(|_| "failed to launch adb")?;
    if !output.status.success() {
        Err(format!(
            "adb returned error code: {:?}\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ))?;
    }
    Ok(String::from_utf8_lossy(&output.stdout).into())
}

pub async fn adb_connect(address: SocketAddr) -> Result<(), String> {
    call_adb(&["connect", &address.to_string()]).await?;
    Ok(())
}

pub async fn adb_pair(address: SocketAddr, pair_code: String) -> Result<(), String> {
    call_adb(&["pair", &address.to_string(), &pair_code]).await?;
    Ok(())
}
