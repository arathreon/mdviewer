//! WebView2 options.

use wry::{WebViewBuilder, WebViewBuilderExtWindows};

/// The origin of all pages. With `with_https_scheme(true)`, WebView2 shows the
/// `mdviewer://` protocol as `https://mdviewer.localhost` (see `configure_webview`).
///
/// The `https` form is a "secure context", so that the page can use all browser
/// features, and the request filter can tell the page from all other addresses.
pub const ORIGIN: &str = "https://mdviewer.localhost";

/// WebView2 browser arguments.
///
/// - Memory (measured: 164 MB to 94 MB): `--disable-gpu`, `--disable-gpu-compositing`,
///   `--renderer-process-limit=1`, `--disable-extensions`.
/// - Security: `--auth-server-allowlist` with a name that does not exist. Without it,
///   WebView2 sends the Windows login (NTLM/Negotiate) automatically to intranet image
///   servers (proved by experiment on 2026-11-06).
/// - The `--disable-features` part replaces the default of wry, so it must stay.
const BROWSER_ARGS: &str = "--disable-gpu --disable-gpu-compositing --renderer-process-limit=1 \
                            --disable-extensions \
                            --auth-server-allowlist=mdviewer.invalid \
                            --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Adds the WebView2 options to the builder: the browser arguments, and the `https`
/// form of the page address ([`ORIGIN`]).
pub fn configure_webview(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    builder
        .with_additional_browser_args(BROWSER_ARGS)
        .with_https_scheme(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_args_block_automatic_windows_login() {
        assert!(BROWSER_ARGS.contains("--auth-server-allowlist=mdviewer.invalid"));
        // `.invalid` is a reserved name that never exists (RFC 2606).
        assert!(
            BROWSER_ARGS.contains("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection")
        );
    }

    #[test]
    fn wry_maps_the_protocol_to_the_origin() {
        // wry changes `<protocol>://` to `https://<protocol>.` on Windows.
        assert_eq!(
            format!("https://{}.localhost", crate::viewer::PROTOCOL),
            ORIGIN
        );
    }
}
