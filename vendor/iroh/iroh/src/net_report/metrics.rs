use serde::{Deserialize, Serialize};

#[cfg(feature = "metrics")]
use iroh_metrics::{Counter, MetricsGroup};

#[cfg(not(feature = "metrics"))]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Counter;

#[cfg(not(feature = "metrics"))]
impl Counter {
    pub fn inc(&self) {}

    pub fn inc_by(&self, _value: u64) {}

    pub fn get(&self) -> u64 {
        0
    }
}

/// Enum of metrics for the module
#[derive(Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "metrics", derive(MetricsGroup))]
#[cfg_attr(feature = "metrics", metrics(name = "net_report"))]
#[non_exhaustive]
pub struct Metrics {
    /// Number of reports executed by net_report, including full reports.
    pub reports: Counter,
    /// Number of full reports executed by net_report
    pub reports_full: Counter,
    /// Number of port mapping attempts.
    pub portmap_attempts: Counter,
    /// Number of times an external address was obtained via port mapping.
    pub portmap_external_address_updated: Counter,
}
