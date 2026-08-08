use rustls::ServerConfig;
use std::{fmt, sync::Arc};

#[derive(Clone)]
pub enum FtpsConfig {
    Off,
    On { tls_config: Arc<ServerConfig> },
    Implicit { tls_config: Arc<ServerConfig> },
}

impl fmt::Debug for FtpsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => write!(f, "Off"),
            Self::On { .. } => write!(f, "On"),
            Self::Implicit { .. } => write!(f, "Implicit"),
        }
    }
}
