//! Resolving the client IP used to key the auth rate limiter (and forwarded to
//! Turnstile). The socket peer is the default and only trustworthy source; a
//! proxy header (`X-Forwarded-For` / `Forwarded`) is honoured ONLY when
//! `TRUST_PROXY_HEADERS` is set, because a directly-exposed server that trusts
//! those headers lets any client spoof its IP and evade the per-IP limits.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, request::Parts},
};

use crate::state::AppState;

/// The resolved client IP, or `None` when it can't be determined (e.g. the
/// in-process test harness drives the router without `ConnectInfo`). A `None` IP
/// means the rate limiter can't key the request and fails open — acceptable,
/// since a real socket always has a peer address in production.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub Option<IpAddr>);

/// Resolve the client IP from the request headers + socket peer, honouring proxy
/// headers only when `trust_proxy` is set.
pub fn resolve_client_ip(
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    trust_proxy: bool,
) -> Option<IpAddr> {
    if trust_proxy {
        // `X-Forwarded-For: client, proxy1, proxy2` — the left-most entry is the
        // original client. (We trust the whole chain because the header only
        // reaches us via a proxy we've been told to trust.)
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
            && let Some(first) = xff.split(',').next()
            && let Ok(ip) = first.trim().parse::<IpAddr>()
        {
            return Some(ip);
        }
        // RFC 7239 `Forwarded: for=1.2.3.4` (optionally quoted / bracketed IPv6).
        if let Some(fwd) = headers.get("forwarded").and_then(|v| v.to_str().ok())
            && let Some(ip) = parse_forwarded_for(fwd)
        {
            return Some(ip);
        }
    }
    peer
}

/// Pull the first `for=` value out of an RFC 7239 `Forwarded` header.
fn parse_forwarded_for(forwarded: &str) -> Option<IpAddr> {
    let first = forwarded.split(',').next()?;
    for part in first.split(';') {
        let part = part.trim();
        if let Some(value) = part
            .strip_prefix("for=")
            .or_else(|| part.strip_prefix("For="))
        {
            let value = value.trim_matches('"');
            // IPv6 in `Forwarded` is bracketed, optionally with a :port.
            let value = value.strip_prefix('[').map_or(value, |rest| {
                rest.split_once(']').map_or(rest, |(inner, _)| inner)
            });
            // Strip a trailing :port for IPv4 (but not IPv6, handled above).
            let candidate = value.rsplit_once(':').map_or(value, |(host, port)| {
                if port.chars().all(|c| c.is_ascii_digit()) && !host.contains(':') {
                    host
                } else {
                    value
                }
            });
            if let Ok(ip) = candidate.parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }
    None
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip());
        Ok(ClientIp(resolve_client_ip(
            &parts.headers,
            peer,
            state.config.trust_proxy_headers,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        use axum::http::HeaderName;
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(k.parse::<HeaderName>().unwrap(), v.parse().unwrap());
        }
        h
    }

    #[test]
    fn peer_is_used_when_proxy_headers_are_not_trusted() {
        let h = headers(&[("x-forwarded-for", "9.9.9.9")]);
        let peer: IpAddr = "1.2.3.4".parse().unwrap();
        // Untrusted: the spoofable header is ignored, the peer wins.
        assert_eq!(resolve_client_ip(&h, Some(peer), false), Some(peer));
    }

    #[test]
    fn leftmost_forwarded_for_is_used_when_trusted() {
        let h = headers(&[("x-forwarded-for", "9.9.9.9, 10.0.0.1, 10.0.0.2")]);
        let peer: IpAddr = "1.2.3.4".parse().unwrap();
        assert_eq!(
            resolve_client_ip(&h, Some(peer), true),
            Some("9.9.9.9".parse().unwrap())
        );
    }

    #[test]
    fn forwarded_header_for_is_parsed_when_trusted() {
        let h = headers(&[("forwarded", "for=9.9.9.9;proto=https")]);
        assert_eq!(
            resolve_client_ip(&h, None, true),
            Some("9.9.9.9".parse().unwrap())
        );
        let h6 = headers(&[("forwarded", "for=\"[2001:db8::1]:4711\"")]);
        assert_eq!(
            resolve_client_ip(&h6, None, true),
            Some("2001:db8::1".parse().unwrap())
        );
    }

    #[test]
    fn falls_back_to_peer_on_a_garbage_header() {
        let h = headers(&[("x-forwarded-for", "not-an-ip")]);
        let peer: IpAddr = "1.2.3.4".parse().unwrap();
        assert_eq!(resolve_client_ip(&h, Some(peer), true), Some(peer));
    }
}
