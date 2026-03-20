use std::collections::HashMap;

use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo};
use tracing::{error, info};

pub struct MDnsService {
    mdns: ServiceDaemon,
    service_info: ServiceInfo,
}

impl MDnsService {
    const SERVICE_TYPE: &str = "_http._tcp.local.";
    const INSTANCE_NAME: &str = "Auto ADB";
    const SERVICE_HOSTNAME: &str = concat!(env!("CARGO_PKG_NAME"), ".local.");

    pub fn register(port: u16) -> Result<Self, String> {
        let mdns = ServiceDaemon::new()
            .map_err(|e| format!("failed to launch mdns service daemon: {e:?}"))?;

        mdns.disable_interface(IfKind::IPv6).ok();
        mdns.disable_interface(IfKind::LoopbackV6).ok();
        mdns.disable_interface(IfKind::LoopbackV4).ok();

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
        mdns.register(service_info.clone())
            .map_err(|e| format!("failed to register mdns service: {e:?}"))?;
        Ok(Self { mdns, service_info })
    }

    pub fn fullname(&self) -> &str {
        self.service_info.get_fullname()
    }

    pub fn restart(&mut self) -> Result<(), String> {
        self.unregister()?;
        self.mdns.shutdown().map_err(|e| format!("{e}"))?;
        self.mdns = ServiceDaemon::new().map_err(|e| format!("{e}"))?;
        self.mdns
            .register(self.service_info.clone())
            .map_err(|e| format!("failed to register mdns service: {e:?}"))?;
        Ok(())
    }

    pub fn unregister(&self) -> Result<(), String> {
        let receiver = self
            .mdns
            .unregister(self.service_info.get_fullname())
            .map_err(|e| format!("failed to unregister: {e:?}"))?;
        while let Ok(event) = receiver.recv() {
            info!("unregister result: {:?}", &event);
        }

        Ok(())
    }
}

impl Drop for MDnsService {
    fn drop(&mut self) {
        if let Err(e) = self.unregister() {
            error!(e);
        }
    }
}
