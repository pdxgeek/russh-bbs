//! The SSRF guard's address rules and the resolver seam (threat-model.md, "SSRF guard").
//!
//! **Allow-list, not block-list.** An address may be connected to only if it is a global unicast
//! address: not loopback, private (RFC 1918, ULA), link-local (169.254.0.0/16, which holds the
//! cloud metadata address, and fe80::/10), carrier-grade NAT (100.64.0.0/10), multicast,
//! unspecified, broadcast, reserved, documentation, benchmarking or any other special-purpose
//! range. IPv6 forms that embed an IPv4 address (IPv4-mapped `::ffff:a.b.c.d`, the deprecated
//! IPv4-compatible `::a.b.c.d`, NAT64 `64:ff9b::/96`, 6to4 `2002::/16`) are judged by the
//! address inside them; Teredo and the other special ranges under `2001::/23` are refused.
//! `fd00:ec2::254` (the AWS IPv6 metadata address) is inside the ULA range.

use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// May a connection go to `ip`?
#[must_use]
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let special = a == 0 // 0.0.0.0/8 "this network", includes unspecified
        || a == 10 // RFC 1918
        || (a == 100 && (64..=127).contains(&b)) // 100.64.0.0/10 CGNAT
        || a == 127 // loopback
        || (a == 169 && b == 254) // link-local, cloud metadata
        || (a == 172 && (16..=31).contains(&b)) // RFC 1918
        || (a == 192 && b == 0 && c == 0) // 192.0.0.0/24 IETF protocol assignments
        || (a == 192 && b == 0 && c == 2) // TEST-NET-1
        || (a == 192 && b == 88 && c == 99) // 6to4 relay anycast (deprecated)
        || (a == 192 && b == 168) // RFC 1918
        || (a == 198 && (18..=19).contains(&b)) // 198.18.0.0/15 benchmarking
        || (a == 198 && b == 51 && c == 100) // TEST-NET-2
        || (a == 203 && b == 0 && c == 113) // TEST-NET-3
        || a >= 224; // multicast 224/4, reserved 240/4, broadcast
    !special
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let seg = ip.segments();
    let bytes = ip.octets();
    // IPv4-mapped (::ffff:0:0/96) and IPv4-compatible (::/96, deprecated): judge the inner
    // address. `::` and `::1` fall out here as 0.0.0.0 and 0.0.0.1.
    if seg[..5] == [0, 0, 0, 0, 0] && (seg[5] == 0xffff || seg[5] == 0) {
        return is_public_v4(Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]));
    }
    // NAT64 well-known prefix 64:ff9b::/96: judge the inner address.
    if seg[0] == 0x0064 && seg[1] == 0xff9b && seg[2..6] == [0, 0, 0, 0] {
        return is_public_v4(Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]));
    }
    // 6to4 2002::/16 embeds the IPv4 address in bytes 2..6.
    if seg[0] == 0x2002 {
        return is_public_v4(Ipv4Addr::new(bytes[2], bytes[3], bytes[4], bytes[5]));
    }
    // Only global unicast 2000::/3 may be connected to. This drops multicast ff00::/8, ULA
    // fc00::/7, link-local fe80::/10, site-local fec0::/10, loopback, unspecified, NAT64 local-use
    // 64:ff9b:1::/48, discard 100::/64 and everything not yet assigned.
    if (seg[0] & 0xe000) != 0x2000 {
        return false;
    }
    let special = (seg[0] == 0x2001 && seg[1] < 0x0200) // 2001::/23 IETF protocol assignments (Teredo, ORCHID, ...)
        || (seg[0] == 0x2001 && seg[1] == 0x0db8) // documentation
        || (seg[0] == 0x3fff && seg[1] < 0x1000); // 3fff::/20 documentation
    !special
}

/// Looks a name up. A trait so a test can make a name resolve to anything; production uses
/// [`SystemResolver`].
pub trait Resolve: Send + Sync {
    /// Every address `host` has, with `port` filled in.
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> impl Future<Output = io::Result<Vec<SocketAddr>>> + Send;
}

/// The operating system's resolver.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

impl Resolve for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        Ok(tokio::net::lookup_host((host, port)).await?.collect())
    }
}

/// Why the guard said no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// A resolved or literal address is not public.
    Address,
    /// The name did not resolve to anything.
    NoAddress,
}

/// The addresses a connection may use.
///
/// `extra_allowed` exists only in test builds and in a build with the `qa-loopback-feeds`
/// feature (never the default): it names exact socket addresses (a local test server) that the
/// range rules would refuse. Release builds have no such field, so there is nothing to switch on.
#[derive(Debug, Clone, Default)]
pub struct Guard {
    #[cfg(any(test, feature = "qa-loopback-feeds"))]
    extra_allowed: Vec<SocketAddr>,
}

impl Guard {
    /// The production guard.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Also allow exactly these socket addresses (a test server on loopback).
    #[cfg(any(test, feature = "qa-loopback-feeds"))]
    #[must_use]
    pub fn allowing(mut self, addrs: impl IntoIterator<Item = SocketAddr>) -> Self {
        self.extra_allowed.extend(addrs);
        self
    }

    /// Check every address. One refused address refuses the whole set: a name that resolves to
    /// a public and a private address is not trusted to pick the public one.
    pub(super) fn check(&self, addrs: &[SocketAddr]) -> Result<(), Refusal> {
        if addrs.is_empty() {
            return Err(Refusal::NoAddress);
        }
        for addr in addrs {
            if !is_public_ip(addr.ip()) && !self.is_extra(*addr) {
                return Err(Refusal::Address);
            }
        }
        Ok(())
    }

    #[cfg(any(test, feature = "qa-loopback-feeds"))]
    fn is_extra(&self, addr: SocketAddr) -> bool {
        self.extra_allowed.contains(&addr)
    }

    #[cfg(not(any(test, feature = "qa-loopback-feeds")))]
    #[allow(clippy::unused_self)]
    fn is_extra(&self, _addr: SocketAddr) -> bool {
        false
    }
}
