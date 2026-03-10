use std::collections::HashMap;

use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo};
use tracing::info;

pub struct MDnsService {
    mdns: ServiceDaemon,
    service_fullname: String,
}

impl MDnsService {
    const SERVICE_TYPE: &str = "_http._tcp.local.";
    const INSTANCE_NAME: &str = "Auto ADB";
    const SERVICE_HOSTNAME: &str = concat!(env!("CARGO_PKG_NAME"), ".local.");

    pub fn register(port: u16) -> Result<Self, String> {
        let mdns = ServiceDaemon::new()
            .map_err(|e| format!("failed to launch mdns service daemon: {e:?}"))?;

        mdns.disable_interface(IfKind::IPv6).ok();

        let service_info = ServiceInfo::new(
            Self::SERVICE_TYPE,
            Self::INSTANCE_NAME,
            Self::SERVICE_HOSTNAME,
            "", // enable_addr_auto
            port,
            HashMap::new(),
        )
        .expect("valid service info")
        .enable_addr_auto();
        let service_fullname = service_info.get_fullname().to_string();
        mdns.register(service_info)
            .map_err(|e| format!("failed to register mdns service: {e:?}"))?;
        Ok(Self {
            mdns,
            service_fullname,
        })
    }

    pub fn unregister(&self) -> Result<(), String> {
        let receiver = self
            .mdns
            .unregister(&self.service_fullname)
            .map_err(|e| format!("failed to unregister: {e:?}"))?;
        while let Ok(event) = receiver.recv() {
            info!("unregister result: {:?}", &event);
        }

        Ok(())
    }
}
