//! DNS resolution for relay host names.

#[cfg(feature = "peer-discovery")]
use std::fmt;
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
};

#[cfg(feature = "peer-discovery")]
use iroh_base::EndpointId;
use n0_error::{AnyError, StackError, StdResultExt, e, stack_error};
use n0_future::{Either, Stream, stream, time::Duration};
use url::Url;

#[cfg(feature = "peer-discovery")]
use crate::{attrs::ParseError, endpoint_info::EndpointInfo};

/// Default DNS query timeout.
pub const DNS_TIMEOUT: Duration = Duration::from_secs(3);

/// The n0 address lookup DNS origin, retained for source compatibility.
pub const N0_DNS_ENDPOINT_ORIGIN_PROD: &str = "dns.iroh.link.";
/// The n0 address lookup DNS origin, retained for source compatibility.
pub const N0_DNS_ENDPOINT_ORIGIN_STAGING: &str = "staging-dns.iroh.link.";

/// Potential errors related to DNS operations.
#[allow(missing_docs)]
#[stack_error(derive, add_meta, from_sources, std_sources)]
#[non_exhaustive]
pub enum DnsError {
    #[error("Request timed out")]
    Timeout {},
    #[error("No response")]
    NoResponse {},
    #[error("Resolve failed, IPv4: {ipv4}, IPv6: {ipv6}")]
    ResolveBoth {
        ipv4: Box<DnsError>,
        ipv6: Box<DnsError>,
    },
    #[error("Missing host")]
    MissingHost {},
    #[error("Failed to resolve")]
    Resolve { source: AnyError },
    #[error("Invalid DNS response: not a query for _iroh.z32encodedpubkey")]
    InvalidResponse {},
}

/// Potential errors related to DNS endpoint address lookups.
#[cfg(feature = "peer-discovery")]
#[allow(missing_docs)]
#[stack_error(derive, add_meta, from_sources)]
#[non_exhaustive]
pub enum LookupError {
    #[error("Malformed txt from lookup")]
    ParseError { source: ParseError },
    #[error("Failed to resolve TXT record")]
    LookupFailed { source: DnsError },
}

/// Error returned when a staggered call fails.
#[stack_error(derive, add_meta)]
#[error("no calls succeeded: [{}]", errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(""))]
pub struct StaggeredError<E: n0_error::StackError + 'static> {
    errors: Vec<E>,
}

impl<E: StackError + 'static> StaggeredError<E> {
    /// Returns an iterator over all encountered errors.
    pub fn iter(&self) -> impl Iterator<Item = &E> {
        self.errors.iter()
    }
}

/// Boxed iterator alias retained for downstream compatibility.
pub type BoxIter<T> = Box<dyn Iterator<Item = T> + Send + 'static>;

/// Record data for a TXT record.
#[cfg(feature = "peer-discovery")]
#[derive(Debug, Clone)]
pub struct TxtRecordData(Box<[Box<[u8]>]>);

#[cfg(feature = "peer-discovery")]
impl TxtRecordData {
    /// Returns an iterator over the character strings contained in this TXT record.
    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        self.0.iter().map(|x| x.as_ref())
    }
}

#[cfg(feature = "peer-discovery")]
impl fmt::Display for TxtRecordData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for value in self.iter() {
            write!(f, "{}", String::from_utf8_lossy(value))?;
        }
        Ok(())
    }
}

#[cfg(feature = "peer-discovery")]
impl FromIterator<Box<[u8]>> for TxtRecordData {
    fn from_iter<T: IntoIterator<Item = Box<[u8]>>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(feature = "peer-discovery")]
impl From<Vec<Box<[u8]>>> for TxtRecordData {
    fn from(value: Vec<Box<[u8]>>) -> Self {
        Self(value.into_boxed_slice())
    }
}

/// A system DNS resolver used by iroh and relay connections.
#[derive(Debug, Clone)]
pub struct DnsResolver;

impl DnsResolver {
    /// Creates a resolver backed by the host operating system.
    pub fn new() -> Self {
        Self
    }

    /// Returns a builder retained for source compatibility.
    pub fn builder() -> Builder {
        Builder
    }

    /// Clears cached resolver state.
    pub fn clear_cache(&self) {}

    /// Refreshes resolver state after a network change.
    pub fn reset(&self) {}

    /// TXT lookups are intentionally unavailable in the ii build.
    #[cfg(feature = "peer-discovery")]
    pub async fn lookup_txt<T: ToString>(
        &self,
        _host: T,
        _timeout: Duration,
    ) -> Result<BoxIter<TxtRecordData>, DnsError> {
        Err(e!(DnsError::NoResponse))
    }

    /// Performs an IPv4 lookup using the host operating system.
    pub async fn lookup_ipv4<T: ToString>(
        &self,
        host: T,
        timeout: Duration,
    ) -> Result<impl Iterator<Item = IpAddr> + use<T>, DnsError> {
        let addrs = lookup(host.to_string(), timeout).await?;
        let ips: Vec<IpAddr> = addrs
            .into_iter()
            .filter(|ip| matches!(ip, IpAddr::V4(_)))
            .collect();
        if ips.is_empty() {
            return Err(e!(DnsError::NoResponse));
        }
        Ok(ips.into_iter())
    }

    /// Performs an IPv6 lookup using the host operating system.
    pub async fn lookup_ipv6<T: ToString>(
        &self,
        host: T,
        timeout: Duration,
    ) -> Result<impl Iterator<Item = IpAddr> + use<T>, DnsError> {
        let addrs = lookup(host.to_string(), timeout).await?;
        let ips: Vec<IpAddr> = addrs
            .into_iter()
            .filter(|ip| matches!(ip, IpAddr::V6(_)))
            .collect();
        if ips.is_empty() {
            return Err(e!(DnsError::NoResponse));
        }
        Ok(ips.into_iter())
    }

    /// Resolves IPv4 and IPv6 in parallel with per-family timeouts.
    pub async fn lookup_ipv4_ipv6<T: ToString>(
        &self,
        host: T,
        timeout: Duration,
    ) -> Result<impl Iterator<Item = IpAddr> + use<T>, DnsError> {
        let host = host.to_string();
        let (ipv4, ipv6) = tokio::join!(
            self.lookup_ipv4(host.clone(), timeout),
            self.lookup_ipv6(host, timeout)
        );
        match (ipv4, ipv6) {
            (Ok(ipv4), Ok(ipv6)) => Ok(LookupIter::Both(ipv4.chain(ipv6))),
            (Ok(ipv4), Err(_)) => Ok(LookupIter::Ipv4(ipv4)),
            (Err(_), Ok(ipv6)) => Ok(LookupIter::Ipv6(ipv6)),
            (Err(ipv4), Err(ipv6)) => Err(e!(DnsError::ResolveBoth {
                ipv4: Box::new(ipv4),
                ipv6: Box::new(ipv6),
            })),
        }
    }

    /// Resolves a hostname from a URL to a single IP address.
    pub async fn resolve_host(
        &self,
        url: &Url,
        prefer_ipv6: bool,
        timeout: Duration,
    ) -> Result<IpAddr, DnsError> {
        match url.host() {
            Some(url::Host::Ipv4(ip)) => Ok(IpAddr::V4(ip)),
            Some(url::Host::Ipv6(ip)) => Ok(IpAddr::V6(ip)),
            Some(url::Host::Domain(host)) => {
                let (ipv4, ipv6) = tokio::join!(
                    self.lookup_ipv4(host, timeout),
                    self.lookup_ipv6(host, timeout)
                );
                let (mut ipv4, mut ipv6) = match (ipv4, ipv6) {
                    (Ok(ipv4), Ok(ipv6)) => (Some(ipv4), Some(ipv6)),
                    (Ok(ipv4), Err(_)) => (Some(ipv4), None),
                    (Err(_), Ok(ipv6)) => (None, Some(ipv6)),
                    (Err(ipv4), Err(ipv6)) => {
                        return Err(e!(DnsError::ResolveBoth {
                            ipv4: Box::new(ipv4),
                            ipv6: Box::new(ipv6),
                        }));
                    }
                };
                let ipv4 = ipv4.as_mut().and_then(Iterator::next);
                let ipv6 = ipv6.as_mut().and_then(Iterator::next);
                if prefer_ipv6 {
                    ipv6.or(ipv4).ok_or_else(|| e!(DnsError::NoResponse))
                } else {
                    ipv4.or(ipv6).ok_or_else(|| e!(DnsError::NoResponse))
                }
            }
            None => Err(e!(DnsError::MissingHost)),
        }
    }

    /// Resolves all addresses for a relay hostname.
    pub fn resolve_host_all<'a>(
        &'a self,
        url: &Url,
        timeout: Duration,
    ) -> impl Stream<Item = Result<IpAddr, DnsError>> + Send + 'a {
        let host = match url.host() {
            Some(url::Host::Ipv4(ip)) => return Either::Left(stream::once(Ok(IpAddr::V4(ip)))),
            Some(url::Host::Ipv6(ip)) => return Either::Left(stream::once(Ok(IpAddr::V6(ip)))),
            Some(url::Host::Domain(host)) => host.to_string(),
            None => return Either::Left(stream::once(Err(e!(DnsError::MissingHost)))),
        };

        enum State<'a> {
            Pending(Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, DnsError>> + Send + 'a>>),
            Ready(std::vec::IntoIter<IpAddr>),
            Done,
        }

        Either::Right(stream::unfold(
            State::Pending(Box::pin(lookup(host, timeout))),
            async |state| match state {
                State::Pending(future) => match future.await {
                    Ok(addresses) => {
                        let mut addresses = addresses.into_iter();
                        match addresses.next() {
                            Some(address) => Some((Ok(address), State::Ready(addresses))),
                            None => Some((Err(e!(DnsError::NoResponse)), State::Done)),
                        }
                    }
                    Err(error) => Some((Err(error), State::Done)),
                },
                State::Ready(mut addresses) => addresses
                    .next()
                    .map(|address| (Ok(address), State::Ready(addresses))),
                State::Done => None,
            },
        ))
    }

    /// Performs a staggered IPv4 lookup.
    pub async fn lookup_ipv4_staggered(
        &self,
        host: impl ToString,
        timeout: Duration,
        delays_ms: &[u64],
    ) -> Result<impl Iterator<Item = IpAddr>, StaggeredError<DnsError>> {
        let host = host.to_string();
        stagger_call(|| self.lookup_ipv4(host.clone(), timeout), delays_ms).await
    }

    /// Performs a staggered IPv6 lookup.
    pub async fn lookup_ipv6_staggered(
        &self,
        host: impl ToString,
        timeout: Duration,
        delays_ms: &[u64],
    ) -> Result<impl Iterator<Item = IpAddr>, StaggeredError<DnsError>> {
        let host = host.to_string();
        stagger_call(|| self.lookup_ipv6(host.clone(), timeout), delays_ms).await
    }

    /// Performs a staggered IPv4/IPv6 lookup.
    pub async fn lookup_ipv4_ipv6_staggered(
        &self,
        host: impl ToString,
        timeout: Duration,
        delays_ms: &[u64],
    ) -> Result<impl Iterator<Item = IpAddr>, StaggeredError<DnsError>> {
        let host = host.to_string();
        stagger_call(|| self.lookup_ipv4_ipv6(host.clone(), timeout), delays_ms).await
    }

    /// Peer endpoint discovery is intentionally unavailable in the ii build.
    #[cfg(feature = "peer-discovery")]
    pub async fn lookup_endpoint_by_id(
        &self,
        _endpoint_id: &EndpointId,
        _origin: &str,
    ) -> Result<EndpointInfo, LookupError> {
        Err(e!(LookupError::LookupFailed {
            source: e!(DnsError::NoResponse),
        }))
    }

    /// Peer endpoint discovery is intentionally unavailable in the ii build.
    #[cfg(feature = "peer-discovery")]
    pub async fn lookup_endpoint_by_domain_name(
        &self,
        _name: &str,
    ) -> Result<EndpointInfo, LookupError> {
        Err(e!(LookupError::LookupFailed {
            source: e!(DnsError::NoResponse),
        }))
    }

    /// Peer endpoint discovery is intentionally unavailable in the ii build.
    #[cfg(feature = "peer-discovery")]
    pub async fn lookup_endpoint_by_domain_name_staggered(
        &self,
        _name: &str,
        _delays_ms: &[u64],
    ) -> Result<EndpointInfo, StaggeredError<LookupError>> {
        Err(e!(StaggeredError {
            errors: vec![e!(LookupError::LookupFailed {
                source: e!(DnsError::NoResponse),
            })],
        }))
    }

    /// Peer endpoint discovery is intentionally unavailable in the ii build.
    #[cfg(feature = "peer-discovery")]
    pub async fn lookup_endpoint_by_id_staggered(
        &self,
        _endpoint_id: &EndpointId,
        _origin: &str,
        _delays_ms: &[u64],
    ) -> Result<EndpointInfo, StaggeredError<LookupError>> {
        Err(e!(StaggeredError {
            errors: vec![e!(LookupError::LookupFailed {
                source: e!(DnsError::NoResponse),
            })],
        }))
    }
}

impl Default for DnsResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder retained for source compatibility. ii always uses system DNS.
#[derive(Debug, Clone, Copy, Default)]
pub struct Builder;

impl Builder {
    /// Uses the host system resolver.
    pub fn with_system_defaults(self) -> Self {
        self
    }

    /// Ignores custom nameservers because ii relies on system DNS.
    pub fn with_nameserver(self, _addr: SocketAddr, _protocol: DnsProtocol) -> Self {
        self
    }

    /// Ignores custom nameservers because ii relies on system DNS.
    pub fn with_nameservers(
        self,
        _nameservers: impl IntoIterator<Item = (SocketAddr, DnsProtocol)>,
    ) -> Self {
        self
    }

    /// Builds a system resolver.
    pub fn build(self) -> DnsResolver {
        DnsResolver::new()
    }
}

/// Protocol marker retained for source compatibility.
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum DnsProtocol {
    /// DNS over UDP.
    #[default]
    Udp,
    /// DNS over TCP.
    Tcp,
}

enum LookupIter<Ipv4, Ipv6> {
    Both(std::iter::Chain<Ipv4, Ipv6>),
    Ipv4(Ipv4),
    Ipv6(Ipv6),
}

impl<Ipv4, Ipv6> Iterator for LookupIter<Ipv4, Ipv6>
where
    Ipv4: Iterator<Item = IpAddr>,
    Ipv6: Iterator<Item = IpAddr>,
{
    type Item = IpAddr;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Both(iter) => iter.next(),
            Self::Ipv4(iter) => iter.next(),
            Self::Ipv6(iter) => iter.next(),
        }
    }
}

async fn lookup(host: String, timeout: Duration) -> Result<Vec<IpAddr>, DnsError> {
    let lookup = tokio::time::timeout(timeout, tokio::net::lookup_host((host.as_str(), 0))).await;
    let addrs = match lookup {
        Ok(result) => result.anyerr()?,
        Err(_) => return Err(e!(DnsError::Timeout)),
    };
    let mut addresses = Vec::new();
    for addr in addrs {
        if !addresses.contains(&addr.ip()) {
            addresses.push(addr.ip());
        }
    }
    if addresses.is_empty() {
        return Err(e!(DnsError::NoResponse));
    }
    Ok(addresses)
}

async fn stagger_call<F, Fut, T, E>(f: F, delays_ms: &[u64]) -> Result<T, StaggeredError<E>>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: StackError + 'static,
{
    let mut errors = Vec::new();
    match f().await {
        Ok(value) => return Ok(value),
        Err(error) => errors.push(error),
    }

    for delay_ms in delays_ms {
        tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
        match f().await {
            Ok(value) => return Ok(value),
            Err(error) => errors.push(error),
        }
    }

    Err(e!(StaggeredError { errors }))
}
