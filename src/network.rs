//! 网络接口变化后的防抖恢复, 避免一次网络切换反复重启 ADB.

use std::time::Duration;

use netwatcher::{UpdateDiff, WatchHandle};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{Instant, sleep_until},
};
use tracing::{info, warn};

use crate::{adb::adb_restart_server, mdns::MDnsService};

const NETWORK_SETTLE_DELAY: Duration = Duration::from_secs(2);

pub struct NetworkMonitor {
    watcher: WatchHandle,
    task: JoinHandle<()>,
}

impl NetworkMonitor {
    pub fn start(mdns: MDnsService) -> Result<Self, netwatcher::Error> {
        // 只保留最近一次事件, 不让短时间内的大量通知堆积.
        let (tx, rx) = watch::channel(None);
        let mut initialized = false;
        let watcher = netwatcher::watch_interfaces(move |update| {
            if should_recover(&mut initialized, &update.diff) {
                info!(diff = ?update.diff, "检测到网络接口变化, 等待网络稳定");
                tx.send_replace(Some(Instant::now()));
            }
        })?;
        let task = tokio::spawn(recover_network(rx, mdns));
        Ok(Self { watcher, task })
    }

    pub async fn stop(self) {
        // 关闭通知后完成正在执行的恢复, 不在 kill/start 之间取消任务.
        drop(self.watcher);
        if let Err(e) = self.task.await {
            warn!(%e, "网络恢复任务退出失败");
        }
    }
}

fn should_recover(initialized: &mut bool, diff: &UpdateDiff) -> bool {
    if !*initialized {
        *initialized = true;
        return false;
    }
    !diff.added.is_empty()
        || !diff.removed.is_empty()
        || diff.modified.values().any(|change| {
            change.hw_addr_changed
                || !change.addrs_added.is_empty()
                || !change.addrs_removed.is_empty()
        })
}

async fn wait_for_settled_change(rx: &mut watch::Receiver<Option<Instant>>) -> bool {
    if rx.changed().await.is_err() {
        return false;
    }
    loop {
        let changed_at = (*rx.borrow_and_update()).expect("网络变化通知包含时间戳");
        tokio::select! {
            biased;
            result = rx.changed() => {
                if result.is_err() {
                    return false;
                }
            }
            _ = sleep_until(changed_at + NETWORK_SETTLE_DELAY) => return true,
        }
    }
}

async fn recover_network(mut rx: watch::Receiver<Option<Instant>>, mut mdns: MDnsService) {
    while wait_for_settled_change(&mut rx).await {
        info!("网络变化已稳定, 正在重启 ADB server");
        match adb_restart_server().await {
            Ok(()) => info!("ADB server 已重启"),
            Err(e) => warn!(%e, "网络变化后重启 ADB server 失败"),
        }

        // mDNS 注销会同步等待 daemon 响应, 不占用异步运行时线程.
        match tokio::task::spawn_blocking(move || {
            let result = mdns.restart();
            (mdns, result)
        })
        .await
        {
            Ok((service, result)) => {
                mdns = service;
                match result {
                    Ok(()) => info!(fullname = mdns.fullname(), "mDNS 服务已重启"),
                    Err(e) => warn!(%e, "网络变化后重启 mDNS 失败"),
                }
            }
            Err(e) => {
                warn!(%e, "mDNS 恢复任务异常退出, 停止网络恢复");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use netwatcher::{InterfaceDiff, IpRecord};

    use super::*;

    #[test]
    fn filters_initial_snapshot_and_empty_diffs() {
        let mut initialized = false;
        let added = UpdateDiff {
            added: vec![1],
            ..Default::default()
        };
        assert!(!should_recover(&mut initialized, &added));
        assert!(!should_recover(&mut initialized, &UpdateDiff::default()));
        assert!(!should_recover(
            &mut initialized,
            &UpdateDiff {
                modified: HashMap::from([(1, InterfaceDiff::default())]),
                ..Default::default()
            }
        ));
        assert!(should_recover(&mut initialized, &added));
        assert!(should_recover(
            &mut initialized,
            &UpdateDiff {
                removed: vec![1],
                ..Default::default()
            }
        ));
        for change in [
            InterfaceDiff {
                hw_addr_changed: true,
                ..Default::default()
            },
            InterfaceDiff {
                addrs_added: vec![IpRecord {
                    ip: "192.0.2.1".parse().unwrap(),
                    prefix_len: 24,
                }],
                ..Default::default()
            },
            InterfaceDiff {
                addrs_removed: vec![IpRecord {
                    ip: "2001:db8::1".parse().unwrap(),
                    prefix_len: 64,
                }],
                ..Default::default()
            },
        ] {
            assert!(should_recover(
                &mut initialized,
                &UpdateDiff {
                    modified: HashMap::from([(1, change)]),
                    ..Default::default()
                }
            ));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn coalesces_bursts_and_keeps_changes_during_recovery() {
        let (tx, mut rx) = watch::channel(None);
        tx.send_replace(Some(Instant::now()));
        {
            let wait = wait_for_settled_change(&mut rx);
            tokio::pin!(wait);
            assert!(
                tokio::time::timeout(Duration::from_secs(1), &mut wait)
                    .await
                    .is_err()
            );
            tx.send_replace(Some(Instant::now()));
            assert!(
                tokio::time::timeout(Duration::from_secs(1), &mut wait)
                    .await
                    .is_err()
            );
            assert!(wait.await);
        }
        // 恢复执行期间到达的新事件必须保留, 不能被刚完成的恢复吞掉.
        tx.send_replace(Some(Instant::now()));
        tx.send_replace(Some(Instant::now()));
        let started = Instant::now();
        assert!(wait_for_settled_change(&mut rx).await);
        assert_eq!(started.elapsed(), NETWORK_SETTLE_DELAY);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), wait_for_settled_change(&mut rx))
                .await
                .is_err()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn closing_monitor_cancels_pending_restart() {
        let (tx, mut rx) = watch::channel(None);
        tx.send_replace(Some(Instant::now()));
        let wait = wait_for_settled_change(&mut rx);
        tokio::pin!(wait);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut wait)
                .await
                .is_err()
        );
        drop(tx);
        assert!(!wait.await);
    }
}
