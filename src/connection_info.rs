//! Client IP address extraction.
//!
//! Forwarding headers (`Forwarded`, `X-Forwarded-For`) are only honoured when the
//! request comes through a trusted proxy, configured with the `TRUST_PROXY`
//! setting (see `config.rs`):
//!
//! * unset            : trust loopback only (`127.0.0.0/8`, `::1/128`)
//! * `false` / `none` : never trust forwarding headers; use the socket peer address
//! * `true` / `*`     : trust every hop (only safe if the backend is unreachable
//!                      except through your proxy)
//! * CIDR list        : comma separated, e.g. `10.0.0.0/8,192.168.0.0/16,::1/128`
//!
//! The client address is found by walking the forwarding chain from the nearest hop
//! (the socket peer) towards the origin and stopping at the first untrusted address.

use crate::config::Config;
use actix_web::{
    dev,
    error::ErrorBadRequest,
    http::header::{HeaderMap, HeaderValue},
    web::Data,
    Error, FromRequest, HttpRequest,
};
use std::future::{ready, Ready};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
};

pub struct ConnectionInfo {
    pub ip: IpAddr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (addr_str, prefix_str) = match s.find('/') {
            Some(i) => (&s[..i], Some(&s[i + 1..])),
            None => (s, None),
        };
        let addr = normalize(IpAddr::from_str(addr_str).ok()?);
        let max = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let prefix = match prefix_str {
            Some(p) => p.parse::<u8>().ok()?,
            None => max,
        };
        if prefix > max {
            return None;
        }
        Some(Cidr { addr, prefix })
    }

    fn contains(&self, ip: &IpAddr) -> bool {
        match (self.addr, normalize(*ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                prefix_match(&net.octets(), &ip.octets(), self.prefix)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                prefix_match(&net.octets(), &ip.octets(), self.prefix)
            }
            _ => false,
        }
    }
}

fn prefix_match(a: &[u8], b: &[u8], prefix: u8) -> bool {
    let full = (prefix / 8) as usize;
    if a[..full] != b[..full] {
        return false;
    }
    let rem = prefix % 8;
    if rem == 0 {
        return true;
    }
    let mask = 0xffu8 << (8 - rem);
    (a[full] & mask) == (b[full] & mask)
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrustProxy {
    None,
    All,
    List(Vec<Cidr>),
}

impl TrustProxy {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        let value = match value.map(str::trim) {
            None | Some("") => return Ok(Self::default_loopback()),
            Some(v) => v,
        };
        match value.to_ascii_lowercase().as_str() {
            "false" | "none" | "0" | "no" => return Ok(TrustProxy::None),
            "true" | "*" | "all" | "1" | "yes" => return Ok(TrustProxy::All),
            _ => {}
        }
        let mut list = Vec::new();
        for item in value.split(',').filter(|x| !x.trim().is_empty()) {
            match Cidr::parse(item) {
                Some(cidr) => list.push(cidr),
                None => return Err(format!("invalid address/CIDR: {}", item.trim())),
            }
        }
        Ok(TrustProxy::List(list))
    }

    pub fn default_loopback() -> Self {
        TrustProxy::List(vec![
            Cidr::parse("127.0.0.0/8").unwrap(),
            Cidr::parse("::1/128").unwrap(),
        ])
    }

    fn is_trusted(&self, ip: &IpAddr) -> bool {
        match self {
            TrustProxy::None => false,
            TrustProxy::All => true,
            TrustProxy::List(list) => list.iter().any(|c| c.contains(ip)),
        }
    }
}

/// Converts IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) to plain IPv4.
fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            if s[0..5] == [0, 0, 0, 0, 0] && s[5] == 0xffff {
                let o = v6.octets();
                IpAddr::V4(Ipv4Addr::new(o[12], o[13], o[14], o[15]))
            } else {
                IpAddr::V6(v6)
            }
        }
        v4 => v4,
    }
}

/// Parses an address as found in forwarding headers:
/// `1.2.3.4`, `1.2.3.4:5678`, `2001:db8::1`, `[2001:db8::1]`, `[2001:db8::1]:5678`,
/// optionally wrapped in double quotes.
fn parse_addr(s: &str) -> Option<IpAddr> {
    let s = s.trim().trim_matches('"');
    if let Ok(ip) = IpAddr::from_str(s) {
        return Some(normalize(ip));
    }
    if let Ok(sock) = SocketAddr::from_str(s) {
        return Some(normalize(sock.ip()));
    }
    if s.starts_with('[') {
        if let Some(end) = s.find(']') {
            if let Ok(ip) = Ipv6Addr::from_str(&s[1..end]) {
                return Some(normalize(IpAddr::V6(ip)));
            }
        }
    }
    None
}

fn header_values<'a>(headers: &'a HeaderMap, name: &str) -> Vec<&'a HeaderValue> {
    headers.get_all(name).collect()
}

/// Returns the forwarding chain, origin first. Entries that cannot be parsed
/// (e.g. `unknown`, obfuscated identifiers) are kept as `None`.
fn forwarded_chain(headers: &HeaderMap) -> Vec<Option<IpAddr>> {
    let mut chain = Vec::new();
    let forwarded = header_values(headers, "forwarded");
    if !forwarded.is_empty() {
        for value in forwarded {
            let value = value.to_str().unwrap_or_default();
            for element in value.split(',') {
                let mut addr = None;
                let mut has_for = false;
                for pair in element.split(';') {
                    let mut kv = pair.splitn(2, '=');
                    let key = kv.next().unwrap_or_default().trim();
                    if key.eq_ignore_ascii_case("for") {
                        has_for = true;
                        addr = parse_addr(kv.next().unwrap_or_default());
                    }
                }
                if has_for {
                    chain.push(addr);
                }
            }
        }
        return chain;
    }
    for value in header_values(headers, "x-forwarded-for") {
        let value = value.to_str().unwrap_or_default();
        for item in value.split(',') {
            if !item.trim().is_empty() {
                chain.push(parse_addr(item));
            }
        }
    }
    chain
}

fn resolve_client_ip(peer: IpAddr, chain: &[Option<IpAddr>], trust: &TrustProxy) -> IpAddr {
    let mut current = normalize(peer);
    for hop in chain.iter().rev() {
        if !trust.is_trusted(&current) {
            break;
        }
        match hop {
            Some(ip) => current = *ip,
            None => break,
        }
    }
    current
}

impl FromRequest for ConnectionInfo {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut dev::Payload) -> Self::Future {
        match req.peer_addr() {
            Some(peer) => {
                let chain = forwarded_chain(req.headers());
                let ip = match req.app_data::<Data<Config>>() {
                    Some(config) => resolve_client_ip(peer.ip(), &chain, &config.trust_proxy),
                    None => resolve_client_ip(peer.ip(), &chain, &TrustProxy::default_loopback()),
                };
                ready(Ok(ConnectionInfo { ip }))
            }
            None => ready(Err(ErrorBadRequest("No IP address"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::header::HeaderName;

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).unwrap()
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.append(
                HeaderName::from_str(k).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn test_parse_addr() {
        assert_eq!(parse_addr("1.2.3.4"), Some(ip("1.2.3.4")));
        assert_eq!(parse_addr(" 1.2.3.4:8080 "), Some(ip("1.2.3.4")));
        assert_eq!(parse_addr("2001:db8::1"), Some(ip("2001:db8::1")));
        assert_eq!(parse_addr("[2001:db8::1]"), Some(ip("2001:db8::1")));
        assert_eq!(parse_addr("\"[2001:db8::1]:443\""), Some(ip("2001:db8::1")));
        assert_eq!(parse_addr("::ffff:1.2.3.4"), Some(ip("1.2.3.4")));
        assert_eq!(parse_addr("unknown"), None);
        assert_eq!(parse_addr("_hidden"), None);
    }

    #[test]
    fn test_cidr() {
        let c = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(c.contains(&ip("10.1.2.3")));
        assert!(!c.contains(&ip("11.0.0.1")));
        assert!(c.contains(&ip("::ffff:10.0.0.1")));
        let c = Cidr::parse("192.168.1.128/25").unwrap();
        assert!(c.contains(&ip("192.168.1.200")));
        assert!(!c.contains(&ip("192.168.1.100")));
        let c = Cidr::parse("2001:db8::/32").unwrap();
        assert!(c.contains(&ip("2001:db8:ffff::1")));
        assert!(!c.contains(&ip("2001:db9::1")));
        assert!(!c.contains(&ip("10.0.0.1")));
        assert_eq!(Cidr::parse("1.2.3.4").unwrap().prefix, 32);
        assert!(Cidr::parse("1.2.3.4/33").is_none());
        assert!(Cidr::parse("nonsense").is_none());
    }

    #[test]
    fn test_trust_proxy_parse() {
        assert_eq!(
            TrustProxy::parse(None).unwrap(),
            TrustProxy::default_loopback()
        );
        assert_eq!(
            TrustProxy::parse(Some("")).unwrap(),
            TrustProxy::default_loopback()
        );
        assert_eq!(TrustProxy::parse(Some("false")).unwrap(), TrustProxy::None);
        assert_eq!(TrustProxy::parse(Some("TRUE")).unwrap(), TrustProxy::All);
        assert!(TrustProxy::parse(Some("10.0.0.0/8,bogus")).is_err());
        match TrustProxy::parse(Some("10.0.0.0/8, ::1")).unwrap() {
            TrustProxy::List(l) => assert_eq!(l.len(), 2),
            _ => panic!(),
        }
    }

    #[test]
    fn test_chain_xff() {
        let h = headers(&[
            ("x-forwarded-for", "1.1.1.1, 2.2.2.2"),
            ("x-forwarded-for", "3.3.3.3"),
        ]);
        assert_eq!(
            forwarded_chain(&h),
            vec![
                Some(ip("1.1.1.1")),
                Some(ip("2.2.2.2")),
                Some(ip("3.3.3.3"))
            ]
        );
        let h = headers(&[
            ("x-forwarded-for", "1.1.1.1"),
            ("x-forwarded-for", "2.2.2.2"),
        ]);
        assert_eq!(
            forwarded_chain(&h),
            vec![Some(ip("1.1.1.1")), Some(ip("2.2.2.2"))]
        );
        let h = headers(&[("x-forwarded-for", "1.1.1.1")]);
        assert_eq!(forwarded_chain(&h), vec![Some(ip("1.1.1.1"))]);
    }

    #[test]
    fn test_chain_forwarded_preferred() {
        let h = headers(&[
            (
                "forwarded",
                "for=\"[2001:db8::1]:1234\";proto=https, for=unknown",
            ),
            ("x-forwarded-for", "9.9.9.9"),
        ]);
        assert_eq!(forwarded_chain(&h), vec![Some(ip("2001:db8::1")), None]);
    }

    #[test]
    fn test_resolve() {
        let chain = vec![
            Some(ip("6.6.6.6")),
            Some(ip("1.1.1.1")),
            Some(ip("10.0.0.2")),
        ];
        // Untrusted peer: headers ignored (spoofing protection)
        assert_eq!(
            resolve_client_ip(ip("5.5.5.5"), &chain, &TrustProxy::default_loopback()),
            ip("5.5.5.5")
        );
        // Trust nothing
        assert_eq!(
            resolve_client_ip(ip("127.0.0.1"), &chain, &TrustProxy::None),
            ip("127.0.0.1")
        );
        // Loopback proxy appends client; client spoofed a leading entry, which is ignored
        let trust = TrustProxy::parse(Some("127.0.0.1,10.0.0.0/8")).unwrap();
        assert_eq!(
            resolve_client_ip(ip("127.0.0.1"), &chain, &trust),
            ip("1.1.1.1")
        );
        // Trust all: leftmost
        assert_eq!(
            resolve_client_ip(ip("127.0.0.1"), &chain, &TrustProxy::All),
            ip("6.6.6.6")
        );
        // Unparsable hop stops at the last trusted address
        let chain = vec![None, Some(ip("10.0.0.2"))];
        assert_eq!(
            resolve_client_ip(ip("127.0.0.1"), &chain, &trust),
            ip("10.0.0.2")
        );
        // IPv6 peer without headers no longer panics
        assert_eq!(
            resolve_client_ip(ip("2001:db8::5"), &[], &TrustProxy::default_loopback()),
            ip("2001:db8::5")
        );
    }
}
