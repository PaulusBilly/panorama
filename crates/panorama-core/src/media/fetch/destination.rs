use super::MediaError;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    error::Error,
    net::{IpAddr, SocketAddr},
};

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

fn filter_addresses(addresses: impl Iterator<Item = SocketAddr>) -> Result<Addrs, MediaError> {
    let addresses: Vec<_> = addresses
        .filter(|address| !forbidden_address(address.ip()))
        .collect();
    if addresses.is_empty() {
        return Err(MediaError::InvalidDestination);
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

pub(super) fn transport_error(error: reqwest::Error) -> MediaError {
    let mut cause: &(dyn Error + 'static) = &error;
    loop {
        if cause.downcast_ref::<MediaError>() == Some(&MediaError::InvalidDestination) {
            return MediaError::InvalidDestination;
        }
        match cause.source() {
            Some(source) => cause = source,
            None => return MediaError::Transport,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_answers_drop_forbidden_addresses_and_reject_empty_results() {
        let addresses = [
            "127.0.0.1:0",
            "[::ffff:127.4.5.6]:0",
            "169.254.169.254:0",
            "[fe80::1]:0",
            "10.0.0.1:0",
        ];
        let parse = || {
            addresses
                .iter()
                .map(|address| address.parse::<SocketAddr>().unwrap())
        };
        assert_eq!(
            filter_addresses(parse()).unwrap().collect::<Vec<_>>(),
            ["10.0.0.1:0".parse().unwrap()]
        );
        assert!(matches!(
            filter_addresses(parse().take(4)),
            Err(MediaError::InvalidDestination)
        ));
        assert!(matches!(
            filter_addresses(std::iter::empty()),
            Err(MediaError::InvalidDestination)
        ));
    }
}
