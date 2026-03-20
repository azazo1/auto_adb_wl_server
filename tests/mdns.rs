use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use tracing::info;

#[tokio::test]
async fn mdns_discover() {
    tracing_subscriber::fmt().init();
    let sd = ServiceDaemon::new().unwrap();
    let receiver = sd.browse("_http._tcp.local.").unwrap();
    let found = Arc::new(AtomicBool::new(false));
    let found_cloned = Arc::clone(&found);
    tokio::spawn(async move {
        while let Ok(evt) = receiver.recv_async().await {
            #[allow(clippy::single_match)]
            match evt {
                ServiceEvent::ServiceResolved(resolved_service) => {
                    info!(
                        "{}: {:#?}",
                        resolved_service.fullname,
                        resolved_service.get_addresses_v4()
                    );
                }
                ServiceEvent::ServiceFound(service_type, fullname) => {
                    info!("found {service_type} : {fullname}");
                    if fullname.to_lowercase().contains("auto adb") {
                        found_cloned.store(true, Ordering::Relaxed);
                    }
                }
                _ => (),
            }
        }
    });
    tokio::time::sleep(Duration::from_secs(3)).await;
    if !found.load(Ordering::Relaxed) {
        panic!("service not found");
    }
}
