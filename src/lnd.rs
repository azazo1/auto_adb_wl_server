use std::path::PathBuf;

use ::lnd::{AnnounceHandle, AnnounceSpec, LndClient};
use tracing::info;

pub const LND_SERVICE_NAME: &str = "_auto-adb-wl._tcp";
pub const DEFAULT_LND_DISPLAY_NAME: &str = "Auto ADB";
const DEFAULT_LND_TTL_SECS: u64 = 30;

pub struct LndAnnounceService {
    handle: Option<AnnounceHandle>,
    node_id: String,
    network_id: Option<String>,
    base_url: String,
}

impl LndAnnounceService {
    pub async fn start(port: u16) -> Result<Option<Self>, String> {
        let Some(base_url) = compiled_lnd_base_url() else {
            return Ok(None);
        };

        let client = LndClient::builder(base_url.to_string())
            .bearer_token(compiled_lnd_bearer_token().to_string())
            .build()
            .map_err(|error| format!("failed to build lnd client: {error}"))?;
        let network_id = client
            .resolve_network_id()
            .map_err(|error| format!("failed to derive lnd network_id: {error}"))?;
        let node_id = resolve_node_id().await?;
        let spec = build_announce_spec(node_id.clone(), network_id.clone(), port);

        let lan_addrs = client
            .resolve_announce_addrs(&spec)
            .map_err(|error| format!("failed to resolve lnd announce addresses: {error}"))?;
        if lan_addrs.is_empty() {
            return Err(
                "failed to resolve lnd announce addresses: no eligible local address found"
                    .to_string(),
            );
        }
        let reachability_scopes = client
            .resolve_reachability_scopes(&spec)
            .map_err(|error| format!("failed to resolve lnd reachability scopes: {error}"))?;
        let mut announcement = spec.clone().into_announcement(lan_addrs);
        announcement.reachability_scopes = reachability_scopes;
        client
            .announce_once(announcement)
            .await
            .map_err(|error| format!("failed to announce to lnd: {error}"))?;
        let handle = client
            .announce_loop(spec)
            .map_err(|error| format!("failed to start lnd announce loop: {error}"))?;

        info!(
            service = LND_SERVICE_NAME,
            %base_url,
            %node_id,
            network_id = %network_id,
            "lnd announce started"
        );

        Ok(Some(Self {
            handle: Some(handle),
            node_id,
            network_id: Some(network_id),
            base_url: base_url.to_string(),
        }))
    }

    pub async fn stop(&mut self) -> Result<(), String> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        handle
            .stop()
            .await
            .map_err(|error| format!("failed to stop lnd announce loop: {error}"))?;
        info!(
            service = LND_SERVICE_NAME,
            base_url = %self.base_url,
            node_id = %self.node_id,
            network_id = ?self.network_id,
            "lnd announce stopped"
        );
        Ok(())
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

async fn resolve_node_id() -> Result<String, String> {
    let path = default_auto_adb_lnd_node_id_path();
    load_or_create_node_id(&path).await.map_err(|error| {
        format!(
            "failed to load lnd node id from {}: {error}",
            path.display()
        )
    })
}

fn default_auto_adb_lnd_node_id_path() -> PathBuf {
    let base = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir);
    let mut path = base;
    path.push(env!("CARGO_PKG_NAME"));
    path.push("lnd_node_id");
    path
}

async fn load_or_create_node_id(path: &std::path::Path) -> Result<String, std::io::Error> {
    match tokio::fs::read_to_string(path).await {
        Ok(value) => Ok(value.trim().to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let node_id = uuid::Uuid::new_v4().to_string();
            tokio::fs::write(path, format!("{node_id}\n")).await?;
            Ok(node_id)
        }
        Err(error) => Err(error),
    }
}

fn compiled_lnd_base_url() -> Option<&'static str> {
    option_env!("AUTO_ADB_WL_LND_BASE_URL")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn compiled_lnd_bearer_token() -> &'static str {
    option_env!("AUTO_ADB_WL_LND_BEARER_TOKEN")
        .map(str::trim)
        .unwrap_or("")
}

fn build_announce_spec(node_id: String, network_id: String, port: u16) -> AnnounceSpec {
    AnnounceSpec::new(node_id, LND_SERVICE_NAME, DEFAULT_LND_DISPLAY_NAME, port)
        .with_ttl_secs(DEFAULT_LND_TTL_SECS)
        .insert_metadata("app", env!("CARGO_PKG_NAME"))
        .insert_metadata("version", env!("CARGO_PKG_VERSION"))
        .with_network_id(network_id)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_LND_DISPLAY_NAME, DEFAULT_LND_TTL_SECS, LND_SERVICE_NAME, build_announce_spec,
    };

    #[test]
    fn build_announce_spec_applies_defaults() {
        let spec = build_announce_spec("node-a".to_string(), "net-a".to_string(), 21300);

        assert_eq!(spec.node_id, "node-a");
        assert_eq!(spec.network_id.as_deref(), Some("net-a"));
        assert_eq!(spec.service, LND_SERVICE_NAME);
        assert_eq!(spec.display_name, DEFAULT_LND_DISPLAY_NAME);
        assert_eq!(spec.port, 21300);
        assert_eq!(spec.ttl_secs, DEFAULT_LND_TTL_SECS);
        assert!(spec.tags.is_empty());
        assert_eq!(
            spec.metadata.get("app").map(String::as_str),
            Some(env!("CARGO_PKG_NAME"))
        );
        assert_eq!(
            spec.metadata.get("version").map(String::as_str),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert!(spec.address_selection.is_none());
    }
}
