use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
};

const CONFIG_DIR_NAME: &str = "ii";
const CONFIG_FILE_NAME: &str = "ii.toml";
const DEFAULT_S3_PROFILE: &str = "cloudflare";
const DEFAULT_WEBDAV_PROFILE: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IiConfig {
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub s3: BTreeMap<String, S3Profile>,
    #[serde(default)]
    pub webdav: BTreeMap<String, WebDavProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Profile {
    pub provider: String,
    #[serde(default)]
    pub account_id: Option<String>,
    pub bucket: String,
    pub endpoint: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default = "default_presign_ttl_seconds")]
    pub presign_ttl_seconds: u32,
    #[serde(default = "default_path_style")]
    pub path_style: bool,
}

#[derive(Debug, Clone)]
pub struct S3ProfileSelection {
    pub path: PathBuf,
    pub config: IiConfig,
    pub profile: S3Profile,
    pub save_after_success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebDavAuth {
    Basic,
    Digest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavProfile {
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(default = "default_prefix")]
    pub remote_dir: String,
    #[serde(default = "default_webdav_auth")]
    pub auth: WebDavAuth,
}

#[derive(Debug, Clone)]
pub struct WebDavProfileSelection {
    pub path: PathBuf,
    pub config: IiConfig,
    pub profile_name: String,
    pub profile: WebDavProfile,
    pub save_after_success: bool,
}

impl S3Profile {
    fn empty_cloudflare() -> Self {
        Self {
            provider: "cloudflare-r2".to_string(),
            account_id: None,
            bucket: String::new(),
            endpoint: String::new(),
            region: "auto".to_string(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            prefix: default_prefix(),
            presign_ttl_seconds: default_presign_ttl_seconds(),
            path_style: default_path_style(),
        }
    }

    pub fn s3_path(&self, object_key: &str) -> String {
        format!("/{}", object_key.trim_start_matches('/'))
    }
}

impl WebDavProfile {
    fn empty() -> Self {
        Self {
            url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_dir: default_prefix(),
            auth: default_webdav_auth(),
        }
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    default_config_path_for(std::env::consts::OS, std::env::current_exe().ok())
}

fn default_config_path_for(os: &str, exe_path: Option<PathBuf>) -> Result<PathBuf> {
    if os == "windows" {
        let exe_path = exe_path.context("find current executable path")?;
        let exe_dir = exe_path
            .parent()
            .context("find current executable directory")?;
        return Ok(exe_dir.join(CONFIG_FILE_NAME));
    }
    Ok(PathBuf::from("/etc")
        .join(CONFIG_DIR_NAME)
        .join(CONFIG_FILE_NAME))
}

pub fn load_config(path: &Path) -> Result<IiConfig> {
    if !path.exists() {
        return Ok(IiConfig::default());
    }
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parse config {}", path.display()))
}

pub fn save_config(path: &Path, config: &IiConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create config dir {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(config).context("serialize config")?;
    std::fs::write(path, raw).with_context(|| format!("write config {}", path.display()))
}

pub fn load_or_prompt_s3_profile() -> Result<S3ProfileSelection> {
    let path = default_config_path()?;
    let config = load_config(&path)?;
    let profile_name = config
        .storage
        .profile
        .clone()
        .unwrap_or_else(|| DEFAULT_S3_PROFILE.to_string());
    load_or_prompt_s3_profile_named(&profile_name)
}

pub fn load_or_prompt_s3_profile_named(profile_name: &str) -> Result<S3ProfileSelection> {
    let path = default_config_path()?;
    let mut config = load_config(&path)?;
    let mut profile = config
        .storage
        .s3
        .get(profile_name)
        .cloned()
        .unwrap_or_else(S3Profile::empty_cloudflare);
    let existed = config.storage.s3.contains_key(profile_name);

    let mut changed = false;
    if profile.provider.trim().is_empty() {
        profile.provider = "cloudflare-r2".to_string();
        changed = true;
    }
    if profile.region.trim().is_empty() {
        profile.region = "auto".to_string();
        changed = true;
    }
    if profile.prefix.trim().is_empty() {
        profile.prefix = default_prefix();
        changed = true;
    }
    if profile.presign_ttl_seconds == 0 {
        profile.presign_ttl_seconds = default_presign_ttl_seconds();
        changed = true;
    }

    if profile.provider == "cloudflare-r2"
        && profile.endpoint.trim().is_empty()
        && let Some(account_id) = profile.account_id.as_deref()
        && !account_id.trim().is_empty()
    {
        profile.endpoint = cloudflare_r2_endpoint(account_id);
        changed = true;
    }

    let missing = missing_cloudflare_fields(&profile);
    if !missing.is_empty() {
        if !std::io::stdin().is_terminal() {
            bail!(
                "S3 config is missing {}. Run `ii send <file> --s3` from an interactive terminal once, or edit {} manually.",
                missing.join(", "),
                path.display()
            );
        }
        println!("ii: Cloudflare R2 is not configured.");
        println!("Open this page:");
        println!("https://dash.cloudflare.com/?to=/:account/r2/api-tokens");
        println!();
        prompt_missing_cloudflare_fields(&mut profile)?;
        changed = true;
    }

    validate_required_profile_fields(&profile, &path)?;
    config.storage.backend = Some("s3".to_string());
    config.storage.profile = Some(profile_name.to_string());
    config
        .storage
        .s3
        .insert(profile_name.to_string(), profile.clone());

    Ok(S3ProfileSelection {
        path,
        config,
        profile,
        save_after_success: changed || !existed,
    })
}

pub fn load_or_prompt_webdav_profile() -> Result<WebDavProfileSelection> {
    load_or_prompt_webdav_profile_named(DEFAULT_WEBDAV_PROFILE)
}

pub fn load_or_prompt_webdav_profile_named(profile_name: &str) -> Result<WebDavProfileSelection> {
    let path = default_config_path()?;
    let mut config = load_config(&path)?;
    let mut profile = config
        .storage
        .webdav
        .get(profile_name)
        .cloned()
        .unwrap_or_else(WebDavProfile::empty);
    let existed = config.storage.webdav.contains_key(profile_name);

    let mut changed = false;
    if profile.remote_dir.trim().is_empty() {
        profile.remote_dir = default_prefix();
        changed = true;
    }

    let missing = missing_webdav_fields(&profile);
    if !missing.is_empty() {
        if !std::io::stdin().is_terminal() {
            bail!(
                "WebDAV config is missing {}. Run `ii send <file> --webdav` or `ii recv <ticket>` from an interactive terminal once, or edit {} manually.",
                missing.join(", "),
                path.display()
            );
        }
        println!("ii: WebDAV is not configured.");
        println!();
        prompt_missing_webdav_fields(&mut profile)?;
        changed = true;
    }

    validate_webdav_profile(&profile, &path)?;
    config.storage.backend = Some("webdav".to_string());
    config.storage.profile = Some(profile_name.to_string());
    config
        .storage
        .webdav
        .insert(profile_name.to_string(), profile.clone());

    Ok(WebDavProfileSelection {
        path,
        config,
        profile_name: profile_name.to_string(),
        profile,
        save_after_success: changed || !existed,
    })
}

pub fn build_webdav_client(profile: &WebDavProfile) -> Result<reqwest_dav::Client> {
    let auth = match profile.auth {
        WebDavAuth::Basic => {
            reqwest_dav::Auth::Basic(profile.username.clone(), profile.password.clone())
        }
        WebDavAuth::Digest => {
            reqwest_dav::Auth::Digest(profile.username.clone(), profile.password.clone())
        }
    };
    reqwest_dav::ClientBuilder::new()
        .set_host(profile.url.clone())
        .set_auth(auth)
        .build()
        .context("create WebDAV client")
}

pub fn build_bucket(profile: &S3Profile) -> Result<Box<s3::Bucket>> {
    let region = s3::Region::Custom {
        region: profile.region.clone(),
        endpoint: profile.endpoint.clone(),
    };
    let credentials = s3::creds::Credentials::new(
        Some(profile.access_key_id.as_str()),
        Some(profile.secret_access_key.as_str()),
        None,
        None,
        None,
    )
    .context("create S3 credentials")?;
    let bucket =
        s3::Bucket::new(&profile.bucket, region, credentials).context("create S3 bucket")?;
    Ok(if profile.path_style {
        bucket.with_path_style()
    } else {
        bucket
    })
}

pub fn normalized_object_key(prefix: &str, random_id: &str, name: &str) -> String {
    let prefix = prefix.trim_matches('/');
    let name = safe_key_component(name);
    if prefix.is_empty() {
        format!("{random_id}-{name}")
    } else {
        format!("{prefix}/{random_id}-{name}")
    }
}

pub fn content_addressed_object_key(prefix: &str, content_md5: [u8; 16]) -> String {
    let prefix = prefix.trim_matches('/');
    let digest = hex::encode(content_md5);
    if prefix.is_empty() {
        digest
    } else {
        format!("{prefix}/{digest}")
    }
}

fn missing_cloudflare_fields(profile: &S3Profile) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if profile.provider == "cloudflare-r2"
        && profile.endpoint.trim().is_empty()
        && profile
            .account_id
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        missing.push("Account ID");
    }
    if profile.bucket.trim().is_empty() {
        missing.push("Bucket");
    }
    if profile.access_key_id.trim().is_empty() {
        missing.push("Access Key ID");
    }
    if profile.secret_access_key.trim().is_empty() {
        missing.push("Secret Access Key");
    }
    missing
}

fn prompt_missing_cloudflare_fields(profile: &mut S3Profile) -> Result<()> {
    if profile.provider == "cloudflare-r2"
        && profile.endpoint.trim().is_empty()
        && profile
            .account_id
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        let account_id = prompt_line("Account ID: ")?;
        profile.endpoint = cloudflare_r2_endpoint(&account_id);
        profile.account_id = Some(account_id);
    }
    if profile.bucket.trim().is_empty() {
        profile.bucket = prompt_line("Bucket: ")?;
    }
    if profile.access_key_id.trim().is_empty() {
        profile.access_key_id = prompt_line("Access Key ID: ")?;
    }
    if profile.secret_access_key.trim().is_empty() {
        profile.secret_access_key = prompt_line("Secret Access Key: ")?;
    }
    Ok(())
}

fn missing_webdav_fields(profile: &WebDavProfile) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if profile.url.trim().is_empty() {
        missing.push("URL");
    }
    if profile.username.trim().is_empty() {
        missing.push("Username");
    }
    if profile.password.trim().is_empty() {
        missing.push("Password");
    }
    missing
}

fn prompt_missing_webdav_fields(profile: &mut WebDavProfile) -> Result<()> {
    if profile.url.trim().is_empty() {
        profile.url = prompt_line("URL: ")?;
    }
    if profile.username.trim().is_empty() {
        profile.username = prompt_line("Username: ")?;
    }
    if profile.password.trim().is_empty() {
        profile.password = prompt_line("Password: ")?;
    }
    Ok(())
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush().context("flush prompt")?;
    let mut input = String::new();
    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    locked.read_line(&mut input).context("read prompt")?;
    let value = input.trim().to_string();
    if value.is_empty() {
        bail!(
            "empty value is not allowed for {}",
            prompt.trim_end_matches(": ")
        );
    }
    Ok(value)
}

fn validate_required_profile_fields(profile: &S3Profile, path: &Path) -> Result<()> {
    let mut missing = Vec::new();
    if profile.endpoint.trim().is_empty() {
        missing.push("endpoint");
    }
    if profile.bucket.trim().is_empty() {
        missing.push("bucket");
    }
    if profile.access_key_id.trim().is_empty() {
        missing.push("access_key_id");
    }
    if profile.secret_access_key.trim().is_empty() {
        missing.push("secret_access_key");
    }
    if !missing.is_empty() {
        bail!(
            "S3 config {} is missing {}",
            path.display(),
            missing.join(", ")
        );
    }
    Ok(())
}

fn validate_webdav_profile(profile: &WebDavProfile, path: &Path) -> Result<()> {
    let mut missing = Vec::new();
    if profile.url.trim().is_empty() {
        missing.push("url");
    }
    if profile.username.trim().is_empty() {
        missing.push("username");
    }
    if profile.password.trim().is_empty() {
        missing.push("password");
    }
    if !missing.is_empty() {
        bail!(
            "WebDAV config {} is missing {}",
            path.display(),
            missing.join(", ")
        );
    }
    let url = url::Url::parse(profile.url.trim()).context("parse WebDAV URL")?;
    if url.scheme() != "http" && url.scheme() != "https" {
        bail!("WebDAV URL must start with http:// or https://");
    }
    Ok(())
}

fn cloudflare_r2_endpoint(account_id: &str) -> String {
    format!(
        "https://{}.r2.cloudflarestorage.com",
        account_id.trim().trim_end_matches('.')
    )
}

fn safe_key_component(name: &str) -> String {
    let cleaned = name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>();
    if cleaned.trim().is_empty() {
        "ii-object".to_string()
    } else {
        cleaned
    }
}

fn default_prefix() -> String {
    "ii/".to_string()
}

fn default_presign_ttl_seconds() -> u32 {
    86_400
}

fn default_path_style() -> bool {
    true
}

fn default_webdav_auth() -> WebDavAuth {
    WebDavAuth::Basic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_ends_with_ii_toml() {
        let path = default_config_path().unwrap();
        assert_eq!(path.file_name().unwrap(), CONFIG_FILE_NAME);
    }

    #[test]
    fn windows_config_path_uses_exe_dir() {
        let path =
            default_config_path_for("windows", Some(PathBuf::from(r"C:\tools\ii.exe"))).unwrap();
        assert_eq!(path, PathBuf::from(r"C:\tools\ii.toml"));
    }

    #[test]
    fn unix_config_path_uses_etc_ii() {
        let path = default_config_path_for("linux", None).unwrap();
        assert_eq!(path, PathBuf::from("/etc/ii/ii.toml"));
    }

    #[test]
    fn object_key_uses_prefix_and_sanitizes_name() {
        let key = normalized_object_key("ii/", "abc", "a\\b:c.txt");
        assert_eq!(key, "ii/abc-a_b_c.txt");
    }

    #[test]
    fn cloudflare_endpoint_is_derived_from_account_id() {
        assert_eq!(
            cloudflare_r2_endpoint("abc"),
            "https://abc.r2.cloudflarestorage.com"
        );
    }

    #[test]
    fn content_key_uses_md5() {
        let key = content_addressed_object_key("ii/", [1; 16]);
        assert_eq!(key, "ii/01010101010101010101010101010101");
    }

    #[test]
    fn webdav_profile_defaults_remote_dir_and_auth() {
        let profile = WebDavProfile::empty();
        assert_eq!(profile.remote_dir, "ii/");
        assert_eq!(profile.auth, WebDavAuth::Basic);
    }
}
