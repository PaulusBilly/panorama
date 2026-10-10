use std::net::{IpAddr, SocketAddr};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use super::ImageError;

// Mirrors media::fetch::destination; share this policy once both changes land.
pub(super) fn forbidden_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_loopback() || address.is_unspecified() || address.is_link_local()
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unicast_link_local()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|address| forbidden_address(address.into()))
        }
    }
}

pub(super) fn filter_addresses(
    addresses: impl Iterator<Item = SocketAddr>,
) -> Result<Addrs, ImageError> {
    let addresses: Vec<_> = addresses
        .filter(|address| !forbidden_address(address.ip()))
        .collect();
    if addresses.is_empty() {
        return Err(ImageError::Network);
    }
    Ok(Box::new(addresses.into_iter()))
}

pub(super) struct SafeDns;

impl Resolve for SafeDns {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addresses = tokio::net::lookup_host((name.as_str(), 0)).await?;
            filter_addresses(addresses).map_err(Into::into)
        })
    }
}
