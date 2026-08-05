#[cfg(test)]
use super::path::{CONFIG_FILE_NAME, default_config_path_for, save_config};
use super::{
    model::*,
    path::{default_config_path, load_config},
    prompt::{prompt_line, prompt_optional_line},
};
use anyhow::{Context, Result, bail};
use std::{
    io::IsTerminal,
    path::{Path, PathBuf},
};

const DEFAULT_S3_PROFILE: &str = "default";
const DEFAULT_R2_PROFILE: &str = "default";
const DEFAULT_AZURE_PROFILE: &str = "default";
const DEFAULT_WEBDAV_PROFILE: &str = "default";
const DEFAULT_FTP_PROFILE: &str = "default";
const DEFAULT_SFTP_PROFILE: &str = "default";

pub fn load_or_prompt_s3_profile() -> Result<S3ProfileSelection> {
    let path = default_config_path()?;
    load_or_prompt_s3_profile_from_path(path, DEFAULT_S3_PROFILE)
}

pub fn load_or_prompt_s3_profile_named(profile_name: &str) -> Result<S3ProfileSelection> {
    let path = default_config_path()?;
    load_or_prompt_s3_profile_from_path(path, profile_name)
}

pub fn load_s3_profile_noninteractive(profile_name: Option<&str>) -> Result<S3ProfileSelection> {
    load_s3_profile_from_path(
        default_config_path()?,
        profile_name.unwrap_or(DEFAULT_S3_PROFILE),
        false,
    )
}

fn load_or_prompt_s3_profile_from_path(
    path: PathBuf,
    profile_name: &str,
) -> Result<S3ProfileSelection> {
    load_s3_profile_from_path(path, profile_name, true)
}

fn load_s3_profile_from_path(
    path: PathBuf,
    profile_name: &str,
    allow_prompt: bool,
) -> Result<S3ProfileSelection> {
    let mut config = load_config(&path)?;
    let existed = config.storage.s3.contains_key(profile_name);
    if !existed
        && profile_name == DEFAULT_S3_PROFILE
        && let Some(legacy_name) = config
            .storage
            .s3
            .iter()
            .find(|(_, profile)| profile.provider == "cloudflare-r2")
            .map(|(name, _)| name)
    {
        bail!(
            "R2 profile `{legacy_name}` must move to [storage.r2.{legacy_name}] and use `ii send <file> --r2`"
        );
    }
    let mut profile = config
        .storage
        .s3
        .get(profile_name)
        .cloned()
        .unwrap_or_else(S3Profile::empty);

    let mut changed = false;
    if profile.provider.trim().is_empty() {
        profile.provider = default_s3_provider();
        changed = true;
    }
    if profile.provider == "cloudflare-r2" {
        bail!(
            "R2 profile `{profile_name}` must move to [storage.r2.{profile_name}] and use `ii send <file> --r2`"
        );
    }
    if profile.region.trim().is_empty() {
        profile.region = "us-east-1".to_string();
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

    let missing = missing_s3_fields(&profile);
    if !missing.is_empty() {
        if !allow_prompt || !std::io::stdin().is_terminal() {
            bail!(
                "S3 config is missing {}. Run `ii send <file> --s3` from an interactive terminal once, or edit {} manually.",
                missing.join(", "),
                path.display()
            );
        }
        println!("ii: S3-compatible storage is not configured.");
        println!();
        prompt_missing_s3_fields(&mut profile, !existed)?;
        changed = true;
    }

    validate_required_profile_fields(&profile, &path)?;
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

pub fn load_or_prompt_r2_profile() -> Result<R2ProfileSelection> {
    load_or_prompt_r2_profile_from_path(default_config_path()?, DEFAULT_R2_PROFILE)
}

pub fn load_or_prompt_r2_profile_named(profile_name: &str) -> Result<R2ProfileSelection> {
    load_or_prompt_r2_profile_from_path(default_config_path()?, profile_name)
}

pub fn load_r2_profile_noninteractive(profile_name: Option<&str>) -> Result<R2ProfileSelection> {
    load_r2_profile_from_path(
        default_config_path()?,
        profile_name.unwrap_or(DEFAULT_R2_PROFILE),
        false,
    )
}

fn load_or_prompt_r2_profile_from_path(
    path: PathBuf,
    profile_name: &str,
) -> Result<R2ProfileSelection> {
    load_r2_profile_from_path(path, profile_name, true)
}

fn load_r2_profile_from_path(
    path: PathBuf,
    profile_name: &str,
    allow_prompt: bool,
) -> Result<R2ProfileSelection> {
    let mut config = load_config(&path)?;
    let existed = config.storage.r2.contains_key(profile_name);
    let mut profile = config
        .storage
        .r2
        .get(profile_name)
        .cloned()
        .unwrap_or_else(R2Profile::empty);
    let mut changed = false;
    if profile.prefix.trim().is_empty() {
        profile.prefix = default_prefix();
        changed = true;
    }
    if profile.presign_ttl_seconds == 0 {
        profile.presign_ttl_seconds = default_presign_ttl_seconds();
        changed = true;
    }
    let missing = missing_r2_fields(&profile);
    if !missing.is_empty() {
        if !allow_prompt || !std::io::stdin().is_terminal() {
            bail!(
                "R2 config is missing {}. Run `ii send <file> --r2` from an interactive terminal once, or edit {} manually.",
                missing.join(", "),
                path.display()
            );
        }
        println!("ii: Cloudflare R2 is not configured.");
        println!("Open this page:");
        println!("https://dash.cloudflare.com/?to=/:account/r2/api-tokens");
        println!();
        prompt_missing_r2_fields(&mut profile)?;
        changed = true;
    }
    validate_r2_profile(&profile).with_context(|| format!("R2 config {}", path.display()))?;
    config
        .storage
        .r2
        .insert(profile_name.to_string(), profile.clone());
    Ok(R2ProfileSelection {
        path,
        config,
        profile,
        save_after_success: changed || !existed,
    })
}

pub fn load_or_prompt_azure_profile() -> Result<AzureProfileSelection> {
    load_or_prompt_azure_profile_from_path(default_config_path()?, DEFAULT_AZURE_PROFILE)
}

pub fn load_or_prompt_azure_profile_named(profile_name: &str) -> Result<AzureProfileSelection> {
    load_or_prompt_azure_profile_from_path(default_config_path()?, profile_name)
}

pub fn load_azure_profile_noninteractive(
    profile_name: Option<&str>,
) -> Result<AzureProfileSelection> {
    load_azure_profile_from_path(
        default_config_path()?,
        profile_name.unwrap_or(DEFAULT_AZURE_PROFILE),
        false,
    )
}

fn load_or_prompt_azure_profile_from_path(
    path: PathBuf,
    profile_name: &str,
) -> Result<AzureProfileSelection> {
    load_azure_profile_from_path(path, profile_name, true)
}

fn load_azure_profile_from_path(
    path: PathBuf,
    profile_name: &str,
    allow_prompt: bool,
) -> Result<AzureProfileSelection> {
    let mut config = load_config(&path)?;
    let existed = config.storage.azure.contains_key(profile_name);
    let mut profile = config
        .storage
        .azure
        .get(profile_name)
        .cloned()
        .unwrap_or_else(AzureProfile::empty);
    let mut changed = false;
    if profile.prefix.trim().is_empty() {
        profile.prefix = default_prefix();
        changed = true;
    }
    if profile.presign_ttl_seconds == 0 {
        profile.presign_ttl_seconds = default_presign_ttl_seconds();
        changed = true;
    }
    let missing = missing_azure_fields(&profile);
    if !missing.is_empty() {
        if !allow_prompt || !std::io::stdin().is_terminal() {
            bail!(
                "Azure config is missing {}. Run `ii send <file> --azure` from an interactive terminal once, or edit {} manually.",
                missing.join(", "),
                path.display()
            );
        }
        println!("ii: Azure Blob Storage is not configured.");
        println!();
        prompt_missing_azure_fields(&mut profile, !existed)?;
        changed = true;
    }
    validate_azure_profile(&profile).with_context(|| format!("Azure config {}", path.display()))?;
    config
        .storage
        .azure
        .insert(profile_name.to_string(), profile.clone());
    Ok(AzureProfileSelection {
        path,
        config,
        profile,
        save_after_success: changed || !existed,
    })
}

pub fn load_or_prompt_webdav_profile() -> Result<WebDavProfileSelection> {
    let path = default_config_path()?;
    load_or_prompt_webdav_profile_from_path(path, DEFAULT_WEBDAV_PROFILE)
}

pub fn load_or_prompt_webdav_profile_named(profile_name: &str) -> Result<WebDavProfileSelection> {
    let path = default_config_path()?;
    load_or_prompt_webdav_profile_from_path(path, profile_name)
}

pub fn load_webdav_profile_noninteractive(profile_name: &str) -> Result<WebDavProfileSelection> {
    load_webdav_profile_from_path(default_config_path()?, profile_name, false)
}

fn load_or_prompt_webdav_profile_from_path(
    path: PathBuf,
    profile_name: &str,
) -> Result<WebDavProfileSelection> {
    load_webdav_profile_from_path(path, profile_name, true)
}

fn load_webdav_profile_from_path(
    path: PathBuf,
    profile_name: &str,
    allow_prompt: bool,
) -> Result<WebDavProfileSelection> {
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
        if !allow_prompt || !std::io::stdin().is_terminal() {
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

    validate_webdav_profile(&profile)
        .with_context(|| format!("WebDAV config {}", path.display()))?;
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

pub fn build_webdav_client(profile: &WebDavProfile) -> Result<crate::webdav::Client> {
    let auth = match profile.auth {
        WebDavAuth::Basic => {
            crate::webdav::Auth::Basic(profile.username.clone(), profile.password.clone())
        }
        WebDavAuth::Digest => {
            crate::webdav::Auth::Digest(profile.username.clone(), profile.password.clone())
        }
    };
    crate::webdav::Client::new(profile.url.clone(), auth).context("create WebDAV client")
}

pub fn load_or_prompt_ftp_profile() -> Result<FtpProfileSelection> {
    let path = default_config_path()?;
    load_or_prompt_ftp_profile_from_path(path, DEFAULT_FTP_PROFILE)
}

pub fn load_or_prompt_ftp_profile_named(profile_name: &str) -> Result<FtpProfileSelection> {
    let path = default_config_path()?;
    load_or_prompt_ftp_profile_from_path(path, profile_name)
}

pub fn load_ftp_profile_noninteractive(profile_name: Option<&str>) -> Result<FtpProfileSelection> {
    load_ftp_profile_from_path(
        default_config_path()?,
        profile_name.unwrap_or(DEFAULT_FTP_PROFILE),
        false,
    )
}

fn load_or_prompt_ftp_profile_from_path(
    path: PathBuf,
    profile_name: &str,
) -> Result<FtpProfileSelection> {
    load_ftp_profile_from_path(path, profile_name, true)
}

fn load_ftp_profile_from_path(
    path: PathBuf,
    profile_name: &str,
    allow_prompt: bool,
) -> Result<FtpProfileSelection> {
    let mut config = load_config(&path)?;
    let mut profile = config
        .storage
        .ftp
        .get(profile_name)
        .cloned()
        .unwrap_or_else(FtpProfile::empty);
    let existed = config.storage.ftp.contains_key(profile_name);
    let mut changed = false;
    if profile.remote_dir.trim().is_empty() {
        profile.remote_dir = default_prefix();
        changed = true;
    }

    let missing = missing_ftp_fields(&profile);
    if !missing.is_empty() {
        if !allow_prompt || !std::io::stdin().is_terminal() {
            bail!(
                "FTP config is missing {}. Run `ii send <file> --ftp` or `ii recv <ticket>` from an interactive terminal once, or edit {} manually.",
                missing.join(", "),
                path.display()
            );
        }
        println!("ii: FTP is not configured.");
        println!();
        prompt_missing_ftp_fields(&mut profile)?;
        changed = true;
    }

    validate_ftp_profile(&profile).with_context(|| format!("FTP config {}", path.display()))?;
    config
        .storage
        .ftp
        .insert(profile_name.to_string(), profile.clone());
    Ok(FtpProfileSelection {
        path,
        config,
        profile_name: profile_name.to_string(),
        profile,
        save_after_success: changed || !existed,
    })
}

pub fn load_or_prompt_sftp_profile() -> Result<SftpProfileSelection> {
    let path = default_config_path()?;
    load_or_prompt_sftp_profile_from_path(path, DEFAULT_SFTP_PROFILE)
}

pub fn load_or_prompt_sftp_profile_named(profile_name: &str) -> Result<SftpProfileSelection> {
    let path = default_config_path()?;
    load_or_prompt_sftp_profile_from_path(path, profile_name)
}

pub fn load_sftp_profile_noninteractive(
    profile_name: Option<&str>,
) -> Result<SftpProfileSelection> {
    load_sftp_profile_from_path(
        default_config_path()?,
        profile_name.unwrap_or(DEFAULT_SFTP_PROFILE),
        false,
    )
}

fn load_or_prompt_sftp_profile_from_path(
    path: PathBuf,
    profile_name: &str,
) -> Result<SftpProfileSelection> {
    load_sftp_profile_from_path(path, profile_name, true)
}

fn load_sftp_profile_from_path(
    path: PathBuf,
    profile_name: &str,
    allow_prompt: bool,
) -> Result<SftpProfileSelection> {
    let mut config = load_config(&path)?;
    let mut profile = config
        .storage
        .sftp
        .get(profile_name)
        .cloned()
        .unwrap_or_else(SftpProfile::empty);
    let existed = config.storage.sftp.contains_key(profile_name);
    let mut changed = false;
    if profile.port == 0 {
        profile.port = default_sftp_port();
        changed = true;
    }
    if profile.remote_dir.trim().is_empty() {
        profile.remote_dir = default_prefix();
        changed = true;
    }

    let missing = missing_sftp_fields(&profile);
    if !missing.is_empty() {
        if !allow_prompt || !std::io::stdin().is_terminal() {
            bail!(
                "SFTP config is missing {}. Run `ii send <file> --sftp` or `ii recv <ticket>` from an interactive terminal once, or edit {} manually.",
                missing.join(", "),
                path.display()
            );
        }
        println!("ii: SFTP is not configured.");
        println!();
        prompt_missing_sftp_fields(&mut profile, !existed)?;
        changed = true;
    }

    validate_sftp_profile(&profile).with_context(|| format!("SFTP config {}", path.display()))?;
    config
        .storage
        .sftp
        .insert(profile_name.to_string(), profile.clone());
    Ok(SftpProfileSelection {
        path,
        config,
        profile_name: profile_name.to_string(),
        profile,
        save_after_success: changed || !existed,
    })
}

pub fn build_bucket(profile: &S3Profile) -> Result<crate::s3::Client> {
    crate::s3::Client::new(
        &profile.bucket,
        &profile.region,
        &profile.endpoint,
        &profile.access_key_id,
        &profile.secret_access_key,
        profile.path_style,
    )
    .context("create S3 bucket")
}

pub fn build_r2_bucket(profile: &R2Profile) -> Result<crate::s3::Client> {
    build_bucket(&r2_as_s3_profile(profile))
}

pub fn r2_as_s3_profile(profile: &R2Profile) -> S3Profile {
    S3Profile {
        provider: "cloudflare-r2".to_string(),
        account_id: Some(profile.account_id.clone()),
        bucket: profile.bucket.clone(),
        endpoint: cloudflare_r2_endpoint(&profile.account_id),
        region: "auto".to_string(),
        access_key_id: profile.access_key_id.clone(),
        secret_access_key: profile.secret_access_key.clone(),
        prefix: profile.prefix.clone(),
        presign_ttl_seconds: profile.presign_ttl_seconds,
        path_style: true,
    }
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
    let digest = hex_lower(content_md5);
    if prefix.is_empty() {
        digest
    } else {
        format!("{prefix}/{digest}")
    }
}

fn hex_lower(bytes: [u8; 16]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for byte in bytes {
        out.push(TABLE[(byte >> 4) as usize] as char);
        out.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    out
}

fn missing_s3_fields(profile: &S3Profile) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if profile.endpoint.trim().is_empty() {
        missing.push("Endpoint");
    }
    if profile.region.trim().is_empty() {
        missing.push("Region");
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

fn prompt_missing_s3_fields(profile: &mut S3Profile, new_profile: bool) -> Result<()> {
    if profile.endpoint.trim().is_empty() {
        profile.endpoint = prompt_line("S3 endpoint (http(s)://host): ")?;
    }
    if new_profile {
        let region = prompt_optional_line("S3 region (default us-east-1): ")?;
        if !region.is_empty() {
            profile.region = region;
        }
    } else if profile.region.trim().is_empty() {
        profile.region = "us-east-1".to_string();
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
    if new_profile {
        let style = prompt_optional_line("Path-style addressing (yes/no, default yes): ")?;
        profile.path_style = match style.as_str() {
            "" | "yes" | "y" | "true" => true,
            "no" | "n" | "false" => false,
            _ => bail!("path-style addressing must be yes or no"),
        };
    }
    Ok(())
}

fn missing_r2_fields(profile: &R2Profile) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if profile.account_id.trim().is_empty() {
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

fn prompt_missing_r2_fields(profile: &mut R2Profile) -> Result<()> {
    if profile.account_id.trim().is_empty() {
        profile.account_id = prompt_line("Account ID: ")?;
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

fn missing_azure_fields(profile: &AzureProfile) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if profile.account_name.trim().is_empty() {
        missing.push("Account name");
    }
    if profile.container.trim().is_empty() {
        missing.push("Container");
    }
    match profile.auth {
        AzureAuth::SharedKey if profile.account_key.trim().is_empty() => {
            missing.push("Account key")
        }
        AzureAuth::Sas if profile.sas_token.trim().is_empty() => missing.push("SAS token"),
        _ => {}
    }
    missing
}

fn prompt_missing_azure_fields(profile: &mut AzureProfile, new_profile: bool) -> Result<()> {
    if new_profile {
        let auth = prompt_optional_line("Authentication (shared-key/sas, default shared-key): ")?;
        profile.auth = match auth.as_str() {
            "" | "shared-key" => AzureAuth::SharedKey,
            "sas" => AzureAuth::Sas,
            _ => bail!("Azure authentication must be shared-key or sas"),
        };
    }
    if profile.account_name.trim().is_empty() {
        profile.account_name = prompt_line("Account name: ")?;
    }
    if profile.container.trim().is_empty() {
        profile.container = prompt_line("Container: ")?;
    }
    if profile.endpoint.is_none() {
        let endpoint = prompt_optional_line("Blob endpoint (optional): ")?;
        profile.endpoint = (!endpoint.is_empty()).then_some(endpoint);
    }
    match profile.auth {
        AzureAuth::SharedKey if profile.account_key.trim().is_empty() => {
            profile.account_key = prompt_line("Account key: ")?;
        }
        AzureAuth::Sas if profile.sas_token.trim().is_empty() => {
            profile.sas_token = prompt_line("Container SAS token: ")?;
        }
        _ => {}
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

fn missing_ftp_fields(profile: &FtpProfile) -> Vec<&'static str> {
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

fn missing_sftp_fields(profile: &SftpProfile) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if profile.host.trim().is_empty() {
        missing.push("Host");
    }
    if profile.username.trim().is_empty() {
        missing.push("Username");
    }
    match profile.auth {
        SftpAuth::Password if profile.password.trim().is_empty() => missing.push("Password"),
        SftpAuth::PrivateKey
            if profile
                .private_key_path
                .as_deref()
                .is_none_or(|path| path.as_os_str().is_empty()) =>
        {
            missing.push("Private key path")
        }
        _ => {}
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

fn prompt_missing_ftp_fields(profile: &mut FtpProfile) -> Result<()> {
    if profile.url.trim().is_empty() {
        profile.url = prompt_line("FTP URL (ftp://host[:port]): ")?;
    }
    if profile.username.trim().is_empty() {
        profile.username = prompt_line("Username: ")?;
    }
    if profile.password.trim().is_empty() {
        profile.password = prompt_line("Password: ")?;
    }
    Ok(())
}

fn prompt_missing_sftp_fields(profile: &mut SftpProfile, new_profile: bool) -> Result<()> {
    if profile.host.trim().is_empty() {
        profile.host = prompt_line("SFTP host: ")?;
    }
    if profile.username.trim().is_empty() {
        profile.username = prompt_line("Username: ")?;
    }
    if new_profile {
        profile.auth = match prompt_line("Authentication (password/private-key): ")?.as_str() {
            "password" => SftpAuth::Password,
            "private-key" => SftpAuth::PrivateKey,
            other => bail!("unsupported SFTP authentication {other}"),
        };
    }
    match profile.auth {
        SftpAuth::Password if profile.password.trim().is_empty() => {
            profile.password = prompt_line("Password: ")?;
        }
        SftpAuth::PrivateKey
            if profile
                .private_key_path
                .as_deref()
                .is_none_or(|path| path.as_os_str().is_empty()) =>
        {
            profile.private_key_path = Some(PathBuf::from(prompt_line("Private key path: ")?));
            let passphrase = prompt_optional_line("Private key passphrase (optional): ")?;
            profile.private_key_passphrase = (!passphrase.is_empty()).then_some(passphrase);
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_s3_profile(profile: &S3Profile) -> Result<()> {
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
        bail!("S3 profile is missing {}", missing.join(", "));
    }
    let url = url::Url::parse(profile.endpoint.trim()).context("parse S3 endpoint")?;
    if url.scheme() != "http" && url.scheme() != "https" {
        bail!("S3 endpoint must start with http:// or https://");
    }
    Ok(())
}

pub fn validate_r2_profile(profile: &R2Profile) -> Result<()> {
    let mut missing = Vec::new();
    if profile.account_id.trim().is_empty() {
        missing.push("account_id");
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
        bail!("R2 profile is missing {}", missing.join(", "));
    }
    Ok(())
}

pub fn validate_azure_profile(profile: &AzureProfile) -> Result<()> {
    let missing = missing_azure_fields(profile);
    if !missing.is_empty() {
        bail!("Azure profile is missing {}", missing.join(", "));
    }
    if let Some(endpoint) = profile.endpoint.as_deref() {
        let url = url::Url::parse(endpoint.trim()).context("parse Azure endpoint")?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            bail!("Azure endpoint must start with http:// or https://");
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("Azure endpoint must not contain user info, a query, or a fragment");
        }
    }
    if profile.auth == AzureAuth::Sas {
        let token = profile.sas_token.trim().trim_start_matches('?');
        let query = url::form_urlencoded::parse(token.as_bytes()).collect::<Vec<_>>();
        if !query
            .iter()
            .any(|(key, value)| key == "sig" && !value.is_empty())
        {
            bail!("Azure SAS token is missing sig");
        }
        let permissions = azure_sas_permissions(token).unwrap_or_default();
        if !permissions.contains('r') || !permissions.contains('w') {
            bail!("Azure SAS token requires read and write permissions");
        }
        if azure_sas_field(token, "sr").as_deref() != Some("c") {
            bail!("Azure SAS token must be a container SAS (sr=c)");
        }
    }
    Ok(())
}

pub fn validate_azure_delete_permission(profile: &AzureProfile) -> Result<()> {
    if profile.auth == AzureAuth::Sas
        && !azure_sas_permissions(&profile.sas_token)
            .is_some_and(|permissions| permissions.contains('d'))
    {
        bail!("Azure SAS token requires delete permission for -d");
    }
    Ok(())
}

fn azure_sas_permissions(token: &str) -> Option<String> {
    azure_sas_field(token, "sp")
}

fn azure_sas_field(token: &str, wanted: &str) -> Option<String> {
    url::form_urlencoded::parse(token.trim().trim_start_matches('?').as_bytes())
        .find(|(key, _)| key == wanted)
        .map(|(_, value)| value.into_owned())
}

fn validate_required_profile_fields(profile: &S3Profile, path: &Path) -> Result<()> {
    validate_s3_profile(profile).with_context(|| format!("S3 config {}", path.display()))
}

pub fn validate_webdav_profile(profile: &WebDavProfile) -> Result<()> {
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
        bail!("WebDAV profile is missing {}", missing.join(", "));
    }
    let url = url::Url::parse(profile.url.trim()).context("parse WebDAV URL")?;
    if url.scheme() != "http" && url.scheme() != "https" {
        bail!("WebDAV URL must start with http:// or https://");
    }
    Ok(())
}

pub fn validate_ftp_profile(profile: &FtpProfile) -> Result<()> {
    let missing = missing_ftp_fields(profile);
    if !missing.is_empty() {
        bail!("FTP profile is missing {}", missing.join(", "));
    }
    let url = url::Url::parse(profile.url.trim()).context("parse FTP URL")?;
    if url.scheme() != "ftp" || url.host_str().is_none() {
        bail!("FTP URL must start with ftp://host[:port]");
    }
    Ok(())
}

pub fn validate_sftp_profile(profile: &SftpProfile) -> Result<()> {
    let missing = missing_sftp_fields(profile);
    if !missing.is_empty() {
        bail!("SFTP profile is missing {}", missing.join(", "));
    }
    if profile.port == 0 {
        bail!("SFTP port must be between 1 and 65535");
    }
    if profile.host.contains('/')
        || profile.host.contains("://")
        || profile.host.contains(char::is_whitespace)
    {
        bail!("SFTP host must be a host name or IP address without a scheme or path");
    }
    if profile.auth == SftpAuth::PrivateKey {
        let key = load_sftp_private_key(profile)?;
        russh::keys::decode_secret_key(&key, profile.private_key_passphrase.as_deref())
            .context("parse SFTP private key")?;
    }
    Ok(())
}

pub fn load_sftp_private_key(profile: &SftpProfile) -> Result<String> {
    let path = profile
        .private_key_path
        .as_ref()
        .filter(|path| !path.as_os_str().is_empty())
        .context("SFTP private_key_path is missing")?;
    std::fs::read_to_string(path)
        .with_context(|| format!("read SFTP private key {}", path.display()))
}

pub fn save_portable_sftp_private_key(profile_name: &str, private_key: &str) -> Result<PathBuf> {
    let config_path = default_config_path()?;
    let config_dir = config_path.parent().context("find ii config directory")?;
    let key_dir = config_dir.join("ii-keys");
    std::fs::create_dir_all(&key_dir)
        .with_context(|| format!("create SFTP key directory {}", key_dir.display()))?;
    let key_path = key_dir.join(format!("{}.key", safe_profile_component(profile_name)));
    std::fs::write(&key_path, private_key)
        .with_context(|| format!("write SFTP private key {}", key_path.display()))?;
    set_private_key_permissions(&key_path)?;
    Ok(key_path)
}

fn safe_profile_component(name: &str) -> String {
    let cleaned = name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>();
    if cleaned.trim().is_empty() {
        "sftp".to_string()
    } else {
        cleaned
    }
}

#[cfg(unix)]
fn set_private_key_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restrict SFTP private key {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_key_permissions(_path: &Path) -> Result<()> {
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

pub(crate) fn default_prefix() -> String {
    "ii/".to_string()
}

pub(crate) fn default_presign_ttl_seconds() -> u32 {
    86_400
}

pub(crate) fn default_s3_provider() -> String {
    "generic-s3".to_string()
}

pub(crate) fn default_azure_auth() -> AzureAuth {
    AzureAuth::SharedKey
}

pub(crate) fn default_path_style() -> bool {
    true
}

pub(crate) fn default_webdav_auth() -> WebDavAuth {
    WebDavAuth::Basic
}

pub(crate) fn default_sftp_auth() -> SftpAuth {
    SftpAuth::Password
}

pub(crate) fn default_sftp_port() -> u16 {
    22
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
            default_config_path_for("windows", Some(PathBuf::from("C:/tools/ii.exe"))).unwrap();
        assert_eq!(path, PathBuf::from("C:/tools").join(CONFIG_FILE_NAME));
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

    #[test]
    fn relay_profile_round_trips_and_retains_self_signed_policy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ii.toml");
        let mut config = IiConfig::default();
        config.relay.insert(
            "实验中继".into(),
            RelayProfile {
                url: "https://relay.example.com".into(),
                accept_self_signed: true,
            },
        );

        save_config(&path, &config).unwrap();
        let loaded = load_config(&path).unwrap();
        let relay = loaded.relay.get("实验中继").unwrap();
        assert_eq!(relay.url, "https://relay.example.com");
        assert!(relay.accept_self_signed);
        relay.validate().unwrap();
    }

    #[test]
    fn relay_profile_rejects_non_https_url() {
        let relay = RelayProfile {
            url: "http://relay.example.com".into(),
            accept_self_signed: false,
        };
        assert!(relay.validate().is_err());
    }

    #[test]
    fn s3_default_does_not_use_legacy_storage_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ii.toml");
        let mut config = IiConfig::default();
        config.storage.backend = Some("webdav".to_string());
        config.storage.profile = Some("default".to_string());
        config.storage.s3.insert(
            DEFAULT_S3_PROFILE.to_string(),
            complete_s3_profile("s3-default"),
        );
        config.storage.webdav.insert(
            DEFAULT_WEBDAV_PROFILE.to_string(),
            complete_webdav_profile("https://dav.example.com"),
        );
        save_config(&path, &config).unwrap();

        let selection = load_or_prompt_s3_profile_from_path(path, DEFAULT_S3_PROFILE).unwrap();

        assert_eq!(selection.profile.bucket, "s3-default");
        assert!(!selection.save_after_success);
    }

    #[test]
    fn s3_rejects_legacy_r2_profile_without_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ii.toml");
        let mut config = IiConfig::default();
        config.storage.s3.insert(
            DEFAULT_S3_PROFILE.to_string(),
            legacy_r2_s3_profile("legacy-cloudflare"),
        );
        save_config(&path, &config).unwrap();

        let err = load_or_prompt_s3_profile_from_path(path, DEFAULT_S3_PROFILE).unwrap_err();

        assert!(err.to_string().contains("use `ii send <file> --r2`"));
    }

    #[test]
    fn s3_default_rejects_legacy_named_r2_profile_without_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ii.toml");
        let mut config = IiConfig::default();
        config
            .storage
            .s3
            .insert("cloudflare".to_string(), legacy_r2_s3_profile("legacy"));
        save_config(&path, &config).unwrap();

        let err = load_or_prompt_s3_profile_from_path(path, DEFAULT_S3_PROFILE).unwrap_err();

        assert!(err.to_string().contains("[storage.r2.cloudflare]"));
    }

    #[test]
    fn r2_profiles_use_their_own_namespace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ii.toml");
        let mut config = IiConfig::default();
        config.storage.r2.insert(
            DEFAULT_R2_PROFILE.to_string(),
            complete_r2_profile("r2-default"),
        );
        config.storage.s3.insert(
            DEFAULT_S3_PROFILE.to_string(),
            complete_s3_profile("s3-default"),
        );
        save_config(&path, &config).unwrap();

        let selection = load_or_prompt_r2_profile_from_path(path, DEFAULT_R2_PROFILE).unwrap();

        assert_eq!(selection.profile.bucket, "r2-default");
        assert!(!selection.save_after_success);
    }

    #[test]
    fn azure_sas_requires_download_and_upload_permissions() {
        let mut profile = complete_azure_sas_profile("sp=rw&sr=c&sig=test");
        validate_azure_profile(&profile).unwrap();
        profile.sas_token = "sp=r&sr=c&sig=test".to_string();
        assert!(validate_azure_profile(&profile).is_err());
        profile.sas_token = "sp=rw&sr=c&sig=test".to_string();
        assert!(validate_azure_delete_permission(&profile).is_err());
        profile.sas_token = "sp=rw&sr=b&sig=test".to_string();
        assert!(validate_azure_profile(&profile).is_err());
    }

    #[test]
    fn s3_named_profile_uses_s3_namespace_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ii.toml");
        let mut config = IiConfig::default();
        config
            .storage
            .s3
            .insert("work".to_string(), complete_s3_profile("s3-work"));
        config.storage.webdav.insert(
            "work".to_string(),
            complete_webdav_profile("https://dav-work.example.com"),
        );
        save_config(&path, &config).unwrap();

        let selection = load_or_prompt_s3_profile_from_path(path, "work").unwrap();

        assert_eq!(selection.profile.bucket, "s3-work");
        assert!(!selection.save_after_success);
    }

    #[test]
    fn webdav_default_does_not_use_legacy_storage_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ii.toml");
        let mut config = IiConfig::default();
        config.storage.backend = Some("s3".to_string());
        config.storage.profile = Some("default".to_string());
        config.storage.s3.insert(
            DEFAULT_S3_PROFILE.to_string(),
            complete_s3_profile("s3-default"),
        );
        config.storage.webdav.insert(
            DEFAULT_WEBDAV_PROFILE.to_string(),
            complete_webdav_profile("https://dav-default.example.com"),
        );
        save_config(&path, &config).unwrap();

        let selection =
            load_or_prompt_webdav_profile_from_path(path, DEFAULT_WEBDAV_PROFILE).unwrap();

        assert_eq!(selection.profile.url, "https://dav-default.example.com");
        assert!(!selection.save_after_success);
    }

    #[test]
    fn webdav_named_profile_uses_webdav_namespace_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ii.toml");
        let mut config = IiConfig::default();
        config
            .storage
            .s3
            .insert("work".to_string(), complete_s3_profile("s3-work"));
        config.storage.webdav.insert(
            "work".to_string(),
            complete_webdav_profile("https://dav-work.example.com"),
        );
        save_config(&path, &config).unwrap();

        let selection = load_or_prompt_webdav_profile_from_path(path, "work").unwrap();

        assert_eq!(selection.profile.url, "https://dav-work.example.com");
        assert!(!selection.save_after_success);
    }

    #[test]
    fn ftp_profile_defaults_and_validation_are_protocol_specific() {
        let profile = FtpProfile::empty();
        assert_eq!(profile.remote_dir, "ii/");
        assert!(validate_ftp_profile(&profile).is_err());

        let valid = FtpProfile {
            url: "ftp://ftp.example.com:2121".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
            remote_dir: "incoming/".to_string(),
        };
        validate_ftp_profile(&valid).unwrap();
        assert!(
            validate_ftp_profile(&FtpProfile {
                url: "https://ftp.example.com".to_string(),
                ..valid
            })
            .is_err()
        );
    }

    #[test]
    fn sftp_profiles_support_password_and_reject_unreadable_private_keys() {
        let password = SftpProfile {
            host: "sftp.example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            remote_dir: "ii/".to_string(),
            auth: SftpAuth::Password,
            password: "pass".to_string(),
            private_key_path: None,
            private_key_passphrase: None,
        };
        validate_sftp_profile(&password).unwrap();

        let private_key = SftpProfile {
            auth: SftpAuth::PrivateKey,
            password: String::new(),
            private_key_path: Some(PathBuf::from("missing-private-key")),
            private_key_passphrase: None,
            ..password
        };
        assert!(validate_sftp_profile(&private_key).is_err());
    }

    #[test]
    fn sftp_private_key_auth_serializes_as_private_key() {
        let mut config = IiConfig::default();
        config.storage.sftp.insert(
            "server".to_string(),
            SftpProfile {
                host: "sftp.example.com".to_string(),
                port: 22,
                username: "user".to_string(),
                remote_dir: "ii/".to_string(),
                auth: SftpAuth::PrivateKey,
                password: String::new(),
                private_key_path: Some(PathBuf::from("id_ed25519")),
                private_key_passphrase: None,
            },
        );
        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("auth = \"private-key\""));
    }

    fn complete_s3_profile(bucket: &str) -> S3Profile {
        S3Profile {
            provider: default_s3_provider(),
            account_id: None,
            bucket: bucket.to_string(),
            endpoint: "https://s3.example.com".to_string(),
            region: "us-east-1".to_string(),
            access_key_id: "key-id".to_string(),
            secret_access_key: "secret".to_string(),
            prefix: default_prefix(),
            presign_ttl_seconds: default_presign_ttl_seconds(),
            path_style: default_path_style(),
        }
    }

    fn legacy_r2_s3_profile(bucket: &str) -> S3Profile {
        S3Profile {
            provider: "cloudflare-r2".to_string(),
            account_id: Some("account".to_string()),
            endpoint: "https://account.r2.cloudflarestorage.com".to_string(),
            region: "auto".to_string(),
            ..complete_s3_profile(bucket)
        }
    }

    fn complete_r2_profile(bucket: &str) -> R2Profile {
        R2Profile {
            account_id: "account".to_string(),
            bucket: bucket.to_string(),
            access_key_id: "key-id".to_string(),
            secret_access_key: "secret".to_string(),
            prefix: default_prefix(),
            presign_ttl_seconds: default_presign_ttl_seconds(),
        }
    }

    fn complete_azure_sas_profile(sas_token: &str) -> AzureProfile {
        AzureProfile {
            auth: AzureAuth::Sas,
            account_name: "account".to_string(),
            container: "files".to_string(),
            endpoint: None,
            account_key: String::new(),
            sas_token: sas_token.to_string(),
            prefix: default_prefix(),
            presign_ttl_seconds: default_presign_ttl_seconds(),
        }
    }

    fn complete_webdav_profile(url: &str) -> WebDavProfile {
        WebDavProfile {
            url: url.to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
            remote_dir: default_prefix(),
            auth: WebDavAuth::Basic,
        }
    }
}
