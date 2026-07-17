//! No-op address lookup types for the ii ticket-only build.
#![allow(missing_docs)]

use std::sync::Arc;

use iroh_base::{EndpointAddr, EndpointId, TransportAddr};
use n0_error::{AnyError, e, stack_error};
use n0_future::{Stream, boxed::BoxStream, stream};

use crate::{Endpoint, endpoint::EndpointError};

pub(crate) const DNS_STAGGERING_MS: &[u64] = &[200, 300, 600, 1000, 2000, 3000];

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct AddrFilter;

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct UserData(Vec<u8>);

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct EndpointData {
    addrs: Vec<TransportAddr>,
    user_data: Option<UserData>,
}

impl EndpointData {
    pub fn new(addrs: Vec<TransportAddr>) -> Self {
        Self {
            addrs,
            user_data: None,
        }
    }

    pub fn set_user_data(&mut self, user_data: Option<UserData>) {
        self.user_data = user_data;
    }
}

#[stack_error(derive, add_meta, from_sources, std_sources)]
#[non_exhaustive]
pub enum AddressLookupBuilderError {
    #[error("Service '{provenance}' error")]
    User {
        provenance: &'static str,
        source: AnyError,
    },
    #[error(transparent)]
    EndpointClosed { source: EndpointError },
}

#[stack_error(derive, add_meta)]
#[non_exhaustive]
#[derive(Clone)]
pub enum AddressLookupFailed {
    #[error("No address lookup configured")]
    NoServiceConfigured,
    #[error("No address lookup results")]
    NoResults { errors: Vec<Error> },
}

#[stack_error(derive, add_meta)]
#[error("Service '{provenance}' failed")]
#[derive(Clone)]
pub struct Error {
    provenance: &'static str,
    #[error(source)]
    source: Arc<AnyError>,
}

pub trait AddressLookup: std::fmt::Debug + Send + Sync + 'static {
    fn publish(&self, _data: &EndpointData) {}

    fn resolve(&self, _endpoint_id: EndpointId) -> Option<BoxStream<Result<Item, Error>>> {
        None
    }
}

pub trait AddressLookupBuilder: Send + Sync + std::fmt::Debug + 'static {
    fn into_address_lookup(
        self,
        endpoint: &Endpoint,
    ) -> Result<impl AddressLookup, AddressLookupBuilderError>;
}

impl<T: AddressLookup> AddressLookupBuilder for T {
    fn into_address_lookup(
        self,
        _endpoint: &Endpoint,
    ) -> Result<impl AddressLookup, AddressLookupBuilderError> {
        Ok(self)
    }
}

pub(crate) trait DynAddressLookupBuilder: Send + Sync + std::fmt::Debug + 'static {
    fn into_address_lookup(
        self: Box<Self>,
        endpoint: &Endpoint,
    ) -> Result<Box<dyn AddressLookup>, AddressLookupBuilderError>;
}

impl<T: AddressLookupBuilder> DynAddressLookupBuilder for T {
    fn into_address_lookup(
        self: Box<Self>,
        endpoint: &Endpoint,
    ) -> Result<Box<dyn AddressLookup>, AddressLookupBuilderError> {
        Ok(Box::new(AddressLookupBuilder::into_address_lookup(
            *self, endpoint,
        )?))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Item {
    endpoint: EndpointAddr,
    provenance: &'static str,
}

impl Item {
    pub fn new(
        endpoint: EndpointAddr,
        provenance: &'static str,
        _last_updated: Option<u64>,
    ) -> Self {
        Self {
            endpoint,
            provenance,
        }
    }

    pub fn endpoint_id(&self) -> EndpointId {
        self.endpoint.id
    }

    pub fn provenance(&self) -> &'static str {
        self.provenance
    }

    pub fn into_endpoint_addr(self) -> EndpointAddr {
        self.endpoint
    }
}

#[derive(Debug, Default, Clone)]
pub struct AddressLookupServices;

impl AddressLookupServices {
    pub fn set_addr_filter(&self, _filter: AddrFilter) {}

    pub fn add(&self, _service: impl AddressLookup + 'static) {}

    pub fn add_boxed(&self, _service: Box<dyn AddressLookup>) {}

    pub fn is_empty(&self) -> bool {
        true
    }

    pub fn len(&self) -> usize {
        0
    }

    pub fn clear(&self) {}

    pub(crate) fn publish(&self, _data: &EndpointData) {}

    pub fn resolve(
        &self,
        _endpoint_id: EndpointId,
    ) -> impl Stream<Item = Result<Result<Item, Error>, AddressLookupFailed>> + use<> {
        stream::once(Err(e!(AddressLookupFailed::NoServiceConfigured)))
    }
}
