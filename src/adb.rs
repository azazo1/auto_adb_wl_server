use std::{ffi::OsStr, future::Future, net::SocketAddr, time::Duration};

use tokio::{process::Command, sync::RwLock};

// 普通请求可以并发, 重启期间独占 server, 避免在 kill/start 之间插入命令.
static ADB_SERVER_LOCK: RwLock<()> = RwLock::const_new(());
const ADB_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

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
    cmd.args(args).kill_on_drop(true);
    let output = tokio::time::timeout(ADB_COMMAND_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "adb 命令执行超时".to_string())?
        .map_err(|e| format!("failed to launch adb: {e}"))?;
    if !output.status.success() {
        Err(format!(
            "adb returned error code: {:?}\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ))?;
    }
    Ok(String::from_utf8_lossy(&output.stdout).into())
}

pub async fn adb_restart_server() -> Result<(), String> {
    let _guard = ADB_SERVER_LOCK.write().await;
    restart_server_with(|command| async move { call_adb(&[command]).await.map(|_| ()) }).await
}

async fn restart_server_with<F, Fut>(mut run: F) -> Result<(), String>
where
    F: FnMut(&'static str) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    for command in ["kill-server", "start-server"] {
        run(command)
            .await
            .map_err(|e| format!("adb {command}: {e}"))?;
    }
    Ok(())
}

pub async fn adb_connect(address: SocketAddr) -> Result<(), String> {
    let _guard = ADB_SERVER_LOCK.read().await;
    let target = address.to_string();
    let output = call_adb(&["connect", &target]).await?;
    validate_connect_output(&target, &output)
}

// adb connect 即使连接失败也可能以 0 退出, 必须额外校验目标地址对应的成功响应.
fn validate_connect_output(target: &str, output: &str) -> Result<(), String> {
    let output = output.trim();
    if output == format!("connected to {target}")
        || output == format!("already connected to {target}")
    {
        return Ok(());
    }
    if output.is_empty() {
        return Err("adb connect returned empty output".into());
    }
    Err(format!("adb connect failed: {output}"))
}

pub async fn adb_pair(address: SocketAddr, pair_code: String) -> Result<(), String> {
    let _guard = ADB_SERVER_LOCK.read().await;
    call_adb(&["pair", &address.to_string(), &pair_code]).await?;
    Ok(())
}

pub async fn adb_disconnect(target: &str) -> Result<(), String> {
    let _guard = ADB_SERVER_LOCK.read().await;
    call_adb(&["disconnect", target]).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{restart_server_with, validate_connect_output};

    #[tokio::test]
    async fn restart_runs_kill_before_start_and_propagates_failures() {
        for failed_command in [None, Some("kill-server"), Some("start-server")] {
            let mut commands = Vec::new();
            let result = restart_server_with(|command| {
                commands.push(command);
                std::future::ready(if failed_command == Some(command) {
                    Err("模拟命令失败".to_string())
                } else {
                    Ok(())
                })
            })
            .await;

            assert_eq!(result.is_ok(), failed_command.is_none());
            let expected = if failed_command == Some("kill-server") {
                vec!["kill-server"]
            } else {
                vec!["kill-server", "start-server"]
            };
            assert_eq!(commands, expected);
        }
    }

    #[test]
    fn connect_accepts_only_success_for_requested_target() {
        for target in ["192.0.2.1:5555", "[2001:db8::1]:5555"] {
            for prefix in ["connected to", "already connected to"] {
                assert!(validate_connect_output(target, &format!("{prefix} {target}\r\n")).is_ok());
            }
        }
    }

    #[test]
    fn connect_rejects_failure_output_even_after_successful_exit() {
        let target = "192.0.2.1:5555";
        for output in [
            "failed to connect to '192.0.2.1:5555': No route to host",
            "failed to connect to '192.0.2.1:5555': Connection refused",
            "failed to authenticate to 192.0.2.1:5555",
        ] {
            let error = validate_connect_output(target, output).unwrap_err();
            assert!(error.contains(output));
        }
    }

    #[test]
    fn connect_rejects_empty_unknown_or_mismatched_output() {
        for output in [
            "",
            "\r\n",
            "unexpected response",
            "connected to 192.0.2.2:5555",
            "already connected to 192.0.2.2:5555",
            "not connected to 192.0.2.1:5555",
        ] {
            assert!(validate_connect_output("192.0.2.1:5555", output).is_err());
        }
    }
}
