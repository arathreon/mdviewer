//! Rules for the network requests of the page.
//!
//! Every request of the web view passes through [`block_reason`] (see `request_filter.rs`).
//! A Markdown file can contain image addresses that point into the local network, for
//! example `https://192.168.1.1/reboot`. Such requests are always blocked. When the user
//! selects "Remote images: Block", all requests to the internet are blocked too.
//!
//! The rules look only at the address, not at the DNS answer. Thus, a public name that
//! points to a private address is not caught. Resolving each name here would make images
//! slower and would still not be safe (the DNS answer can change before the browser asks).

use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

use crate::settings::RemoteImages;

/// The host of the page itself. The viewer answers these requests; they never go to
/// the network.
pub const PAGE_HOST: &str = "mdviewer.localhost";

/// Name endings that are used only in local networks. None of them is a public
/// top-level domain (`.local`: mDNS, `.home.arpa`: RFC 8375, `.internal`: ICANN 2024).
const LOCAL_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "lan",
    "internal",
    "intranet",
    "corp",
    "home",
    "home.arpa",
];

/// Gives the reason why a request must be blocked, or `None` if it can go out.
pub fn block_reason(address: &str, remote: RemoteImages) -> Option<&'static str> {
    let Ok(url) = Url::parse(address) else {
        return Some("address is not valid");
    };
    match url.scheme() {
        // These never use the network.
        "data" | "blob" => return None,
        "http" | "https" => {}
        _ => return Some("protocol is not allowed"),
    }
    let Some(host) = url.host() else {
        return Some("address has no host");
    };
    if url.scheme() == "https" && host == Host::Domain(PAGE_HOST) {
        return None;
    }
    if remote == RemoteImages::Block {
        return Some("remote images are off");
    }
    let local = match host {
        Host::Ipv4(ip) => is_local_ipv4(ip),
        Host::Ipv6(ip) => is_local_ipv6(ip),
        Host::Domain(name) => is_local_name(name),
    };
    local.then_some("local network address")
}

/// Addresses that are not on the public internet.
fn is_local_ipv4(ip: Ipv4Addr) -> bool {
    let [first, second, ..] = ip.octets();
    ip.is_private()            // 10/8, 172.16/12, 192.168/16
        || ip.is_loopback()    // 127/8
        || ip.is_link_local()  // 169.254/16, also cloud metadata servers
        || ip.is_broadcast()
        || first == 0          // 0/8: "this network"; 0.0.0.0 can reach this computer
        || (first == 100 && (64..128).contains(&second)) // 100.64/10: carrier NAT, VPNs
}

/// IPv6 addresses that are not on the public internet, also IPv4 addresses inside them.
fn is_local_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    // An IPv4 address inside an IPv6 address: ::ffff:a.b.c.d (mapped),
    // ::a.b.c.d (compatible, old) and 64:ff9b::a.b.c.d (NAT64).
    let embedded = (segments[..5] == [0; 5] && (segments[5] == 0xffff || segments[5] == 0))
        || segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0];
    if embedded {
        let [.., high, low] = segments;
        let v4 = Ipv4Addr::from((u32::from(high) << 16) | u32::from(low));
        return is_local_ipv4(v4);
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || (segments[0] & 0xfe00) == 0xfc00 // fc00::/7: unique local
        || (segments[0] & 0xffc0) == 0xfe80 // fe80::/10: link-local
        || (segments[0] & 0xffc0) == 0xfec0 // fec0::/10: site-local (old)
}

/// Names without a dot (`router`, `nas`) are intranet names for Windows.
fn is_local_name(name: &str) -> bool {
    let name = name.trim_end_matches('.');
    !name.contains('.')
        || LOCAL_SUFFIXES.iter().any(|suffix| {
            name == *suffix
                || name
                    .strip_suffix(suffix)
                    .is_some_and(|rest| rest.ends_with('.'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(address: &str) -> Option<&'static str> {
        block_reason(address, RemoteImages::Show)
    }

    const LOCAL: Option<&str> = Some("local network address");

    #[test]
    fn public_addresses_are_allowed() {
        for address in [
            "https://img.shields.io/badge/build-passing-green",
            "https://raw.githubusercontent.com/user/repo/main/image.png",
            "https://8.8.8.8/x.png",
            "https://[2001:4860:4860::8888]/x.png",
            "https://172.32.0.1/x.png",
            "https://100.128.0.1/x.png",
            "https://localhost.example.com/x.png",
            "https://example.local.com/x.png",
        ] {
            assert_eq!(shown(address), None, "{address}");
        }
    }

    #[test]
    fn private_ipv4_addresses_are_blocked() {
        for address in [
            "https://10.0.0.1/",
            "https://172.16.0.1/",
            "https://172.31.255.255/",
            "https://192.168.1.1/reboot",
            "https://192.168.1.1:8443/",
            "https://user:password@192.168.0.1/",
            "https://127.0.0.1/",
            "https://169.254.169.254/latest/meta-data/",
            "https://0.0.0.0/",
            "https://255.255.255.255/",
            "https://100.64.0.1/",
            "https://100.127.255.255/",
            "http://192.168.1.1/",
        ] {
            assert_eq!(shown(address), LOCAL, "{address}");
        }
    }

    #[test]
    fn unusual_ipv4_forms_are_read_like_the_browser_reads_them() {
        // All of these mean 127.0.0.1 or 192.168.1.1 for Chromium too.
        for address in [
            "https://0x7f.1/",
            "https://2130706433/",
            "https://127.1/",
            "https://0177.0.0.1/",
            "https://0xc0.0xa8.1.1/",
            "https://3232235777/",
        ] {
            assert_eq!(shown(address), LOCAL, "{address}");
        }
    }

    #[test]
    fn local_ipv6_addresses_are_blocked() {
        for address in [
            "https://[::1]/",
            "https://[::]/",
            "https://[fd00::1]/",
            "https://[fc00::1]/",
            "https://[fe80::1]/",
            "https://[fec0::1]/",
            "https://[::ffff:192.168.1.1]/",
            "https://[::ffff:c0a8:101]/",
            "https://[::ffff:127.0.0.1]/",
            "https://[::127.0.0.1]/",
            "https://[64:ff9b::10.0.0.1]/",
        ] {
            assert_eq!(shown(address), LOCAL, "{address}");
        }
        assert_eq!(shown("https://[::ffff:8.8.8.8]/"), None);
        assert_eq!(shown("https://[64:ff9b::8.8.8.8]/"), None);
    }

    #[test]
    fn local_names_are_blocked() {
        for address in [
            "https://localhost/",
            "https://LOCALHOST/",
            "https://localhost./",
            "https://router/",
            "https://Router./admin",
            "https://mylan/x.png.example.org/",
            "https://nas.local/",
            "https://printer.lan/",
            "https://wiki.corp/",
            "https://server.internal/",
            "https://box.home/",
            "https://gateway.home.arpa/",
            "https://app.intranet/",
            "https://evil.localhost/",
            "https://mdviewer.localhost.localhost/",
        ] {
            assert_eq!(shown(address), LOCAL, "{address}");
        }
    }

    #[test]
    fn the_page_itself_is_allowed_only_over_https() {
        assert_eq!(shown("https://mdviewer.localhost/"), None);
        assert_eq!(shown("https://mdviewer.localhost/image.png"), None);
        assert_eq!(shown("https://MDVIEWER.localhost/"), None);
        assert_eq!(shown("http://mdviewer.localhost/"), LOCAL);
    }

    #[test]
    fn data_and_blob_addresses_are_allowed() {
        assert_eq!(shown("data:image/png;base64,iVBORw0KGgo="), None);
        assert_eq!(shown("blob:https://mdviewer.localhost/1234"), None);
    }

    #[test]
    fn other_protocols_and_bad_addresses_are_blocked() {
        let protocol = Some("protocol is not allowed");
        assert_eq!(shown("file:///C:/Windows/win.ini"), protocol);
        assert_eq!(shown("file://server/share/x.png"), protocol);
        assert_eq!(shown("ftp://example.com/x.png"), protocol);
        assert_eq!(shown("ws://example.com/"), protocol);
        assert_eq!(shown("not an address"), Some("address is not valid"));
        assert_eq!(shown(""), Some("address is not valid"));
    }

    #[test]
    fn block_setting_blocks_all_remote_requests_but_not_the_page() {
        let off = Some("remote images are off");
        let blocked = |address| block_reason(address, RemoteImages::Block);
        assert_eq!(blocked("https://img.shields.io/badge/x"), off);
        assert_eq!(blocked("https://8.8.8.8/"), off);
        assert_eq!(blocked("https://192.168.1.1/"), off);
        assert_eq!(blocked("https://mdviewer.localhost/"), None);
        assert_eq!(blocked("data:image/png;base64,iVBORw0KGgo="), None);
    }
}
