use super::{
    default_azure_auth, default_path_style, default_prefix, default_presign_ttl_seconds,
    default_s3_provider, default_sftp_auth, default_sftp_port, default_webdav_auth,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IiConfig {
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub relay: BTreeMap<String, RelayProfile>,
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
    pub r2: BTreeMap<String, R2Profile>,
    #[serde(default)]
    pub azure: BTreeMap<String, AzureProfile>,
    #[serde(default)]
    pub webdav: BTreeMap<String, WebDavProfile>,
    #[serde(default)]
    pub ftp: BTreeMap<String, FtpProfile>,
    #[serde(default)]
    pub sftp: BTreeMap<String, SftpProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Profile {
    #[serde(default = "default_s3_provider")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R2Profile {
    pub account_id: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default = "default_presign_ttl_seconds")]
    pub presign_ttl_seconds: u32,
}

#[derive(Debug, Clone)]
pub struct R2ProfileSelection {
    pub path: PathBuf,
    pub config: IiConfig,
    pub profile: R2Profile,
    pub save_after_success: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AzureAuth {
    SharedKey,
    Sas,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureProfile {
    #[serde(default = "default_azure_auth")]
    pub auth: AzureAuth,
    pub account_name: String,
    pub container: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub account_key: String,
    #[serde(default)]
    pub sas_token: String,
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default = "default_presign_ttl_seconds")]
    pub presign_ttl_seconds: u32,
}

#[derive(Debug, Clone)]
pub struct AzureProfileSelection {
    pub path: PathBuf,
    pub config: IiConfig,
    pub profile: AzureProfile,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpProfile {
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(default = "default_prefix")]
    pub remote_dir: String,
}

#[derive(Debug, Clone)]
pub struct FtpProfileSelection {
    pub path: PathBuf,
    pub config: IiConfig,
    pub profile_name: String,
    pub profile: FtpProfile,
    pub save_after_success: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SftpAuth {
    #[serde(rename = "password")]
    Password,
    #[serde(rename = "private-key")]
    PrivateKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpProfile {
    pub host: String,
    #[serde(default = "default_sftp_port")]
    pub port: u16,
    pub username: String,
    #[serde(default = "default_prefix")]
    pub remote_dir: String,
    #[serde(default = "default_sftp_auth")]
    pub auth: SftpAuth,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub private_key_path: Option<PathBuf>,
    #[serde(default)]
    pub private_key_passphrase: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SftpProfileSelection {
    pub path: PathBuf,
    pub config: IiConfig,
    pub profile_name: String,
    pub profile: SftpProfile,
    pub save_after_success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RelayProfile {
    pub url: String,
    #[serde(default)]
    pub accept_self_signed: bool,
}

impl RelayProfile {
    pub fn validate(&self) -> Result<()> {
        let url = url::Url::parse(self.url.trim()).context("parse relay URL")?;
        if url.scheme() != "https" || url.host_str().is_none() {
            bail!("relay URL must be https://host[:port]");
        }
        Ok(())
    }
}

impl S3Profile {
    pub fn empty() -> Self {
        Self::new_empty()
    }

    fn new_empty() -> Self {
        Self {
            provider: default_s3_provider(),
            account_id: None,
            bucket: String::new(),
            endpoint: String::new(),
            region: "us-east-1".to_string(),
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

impl R2Profile {
    pub fn empty() -> Self {
        Self {
            account_id: String::new(),
            bucket: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            prefix: default_prefix(),
            presign_ttl_seconds: default_presign_ttl_seconds(),
        }
    }
}

impl AzureProfile {
    pub fn empty() -> Self {
        Self {
            auth: default_azure_auth(),
            account_name: String::new(),
            container: String::new(),
            endpoint: None,
            account_key: String::new(),
            sas_token: String::new(),
            prefix: default_prefix(),
            presign_ttl_seconds: default_presign_ttl_seconds(),
        }
    }
}

impl WebDavProfile {
    pub fn empty() -> Self {
        Self::new_empty()
    }

    fn new_empty() -> Self {
        Self {
            url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_dir: default_prefix(),
            auth: default_webdav_auth(),
        }
    }
}

impl FtpProfile {
    pub fn empty() -> Self {
        Self {
            url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_dir: default_prefix(),
        }
    }
}

impl SftpProfile {
    pub fn empty() -> Self {
        Self {
            host: String::new(),
            port: default_sftp_port(),
            username: String::new(),
            remote_dir: default_prefix(),
            auth: default_sftp_auth(),
            password: String::new(),
            private_key_path: None,
            private_key_passphrase: None,
        }
    }
}
