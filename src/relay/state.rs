use anyhow::{Context, Result, bail};
use rcgen::generate_simple_self_signed;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const RELAY_STATE_VERSION: u8 = 1;
const CERT_FILE_NAME: &str = "relay-cert.pem";
const KEY_FILE_NAME: &str = "relay-key.pem";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RelayState {
    pub(crate) version: u8,
    pub(crate) public_url: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RelayPaths {
    pub(crate) config_path: PathBuf,
    pub(crate) cert_path: PathBuf,
    pub(crate) key_path: PathBuf,
}

pub fn default_config_path() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().context("locate ii.exe")?;
        let dir = exe.parent().context("locate ii.exe directory")?;
        return Ok(dir.join("relay.toml"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(PathBuf::from("/etc/ii/relay.toml"))
    }
}

pub(crate) fn relay_url_for_domain(domain: &str, port: u16, default_tls_port: u16) -> String {
    if port == default_tls_port {
        format!("https://{domain}")
    } else {
        format!("https://{domain}:{port}")
    }
}

pub(crate) fn paths(config_path: PathBuf) -> RelayPaths {
    let parent = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    RelayPaths {
        cert_path: parent.join(CERT_FILE_NAME),
        key_path: parent.join(KEY_FILE_NAME),
        config_path,
    }
}

pub(crate) fn load_or_create(
    paths: &RelayPaths,
    public_url: &iroh::RelayUrl,
) -> Result<RelayState> {
    if paths.config_path.exists() {
        let text = fs::read_to_string(&paths.config_path)
            .with_context(|| format!("read relay state {}", paths.config_path.display()))?;
        let state: RelayState = toml::from_str(&text).map_err(|err| {
            anyhow::anyhow!(
                "unsupported relay.toml for the self-signed relay mode; remove relay.toml, relay-cert.pem, and relay-key.pem together, then run ii relay --public <https-url>: {err}"
            )
        })?;
        if state.version != RELAY_STATE_VERSION {
            bail!("unsupported relay state version {}", state.version);
        }
        if state.public_url != public_url.as_str() {
            bail!(
                "relay state is bound to {}; requested {}. Remove relay.toml, relay-cert.pem, and relay-key.pem together to create a new relay identity",
                state.public_url,
                public_url
            );
        }
        match (paths.cert_path.exists(), paths.key_path.exists()) {
            (true, true) => return Ok(state),
            _ => bail!(
                "relay certificate state is incomplete: expected {} and {}",
                paths.cert_path.display(),
                paths.key_path.display()
            ),
        }
    }

    if paths.cert_path.exists() || paths.key_path.exists() {
        bail!(
            "relay certificate state is incomplete: relay.toml is missing but certificate material exists beside it"
        );
    }
    if let Some(parent) = paths.config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create relay state directory {}", parent.display()))?;
    }

    let host = public_url
        .host_str()
        .context("public relay URL must include a host")?;
    let certified_key = generate_simple_self_signed(vec![host.to_string()])
        .context("generate self-signed relay certificate")?;
    fs::write(&paths.cert_path, certified_key.cert.pem())
        .with_context(|| format!("write relay certificate {}", paths.cert_path.display()))?;
    fs::write(&paths.key_path, certified_key.signing_key.serialize_pem())
        .with_context(|| format!("write relay key {}", paths.key_path.display()))?;
    set_private_key_permissions(&paths.key_path)?;

    let state = RelayState {
        version: RELAY_STATE_VERSION,
        public_url: public_url.to_string(),
    };
    let text = toml::to_string_pretty(&state).context("serialize relay state")?;
    fs::write(&paths.config_path, text)
        .with_context(|| format!("write relay state {}", paths.config_path.display()))?;
    Ok(state)
}

#[cfg(unix)]
fn set_private_key_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set private key permissions {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_key_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
