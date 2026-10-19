use std::fmt;

use url::{Host, Url};

use super::InstallError;

/// Validated manifest URL retaining pasted path/query bytes and redacting Debug.
#[derive(Clone)]
pub struct AddonUrl {
    raw: String,
    pub(crate) parsed: Url,
}

impl AddonUrl {
    /// Trims edges, rewrites stremio to HTTPS and rejects unsafe manifest URLs.
    pub fn parse(input: &str) -> Result<Self, InstallError> {
        let input = input.trim();
        if input.chars().count() > 2048
            || input.chars().any(|c| c.is_control() || c.is_whitespace())
            || input.contains('\\')
        {
            return Err(InstallError::InvalidUrl);
        }
        let (scheme, rest) = input.split_once("://").ok_or(InstallError::InvalidUrl)?;
        let raw = if scheme.eq_ignore_ascii_case("stremio") {
            format!("https://{rest}")
        } else if scheme.eq_ignore_ascii_case("https") {
            input.to_owned()
        } else {
            return Err(InstallError::InvalidUrl);
        };
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .ok_or(InstallError::InvalidUrl)?;
        let parsed = Url::parse(&raw).map_err(|_| InstallError::InvalidUrl)?;
        if authority.contains('@')
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
            || !parsed.path().ends_with("/manifest.json")
            || forbidden_host(&parsed)
        {
            return Err(InstallError::InvalidUrl);
        }
        let path = rest
            .get(authority.len()..)
            .ok_or(InstallError::InvalidUrl)?
            .split(['?', '#'])
            .next()
            .ok_or(InstallError::InvalidUrl)?;
        if path.split('/').any(|segment| {
            let dots = segment.to_ascii_lowercase().replace("%2e", ".");
            dots == "." || dots == ".."
        }) {
            return Err(InstallError::InvalidUrl);
        }
        Ok(Self { raw, parsed })
    }

    /// Borrows the validated text; may contain configuration secrets, never log it.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

pub(crate) fn forbidden_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback() || ip.is_link_local() || ip.is_unspecified(),
        Some(Host::Ipv6(ip)) => {
            ip.is_loopback()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|ip| ip.is_loopback() || ip.is_link_local() || ip.is_unspecified())
        }
        Some(Host::Domain(_)) => false,
        None => true,
    }
}

impl fmt::Debug for AddonUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AddonUrl { .. }")
    }
}
