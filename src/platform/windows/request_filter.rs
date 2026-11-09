//! Sends every request of the web view through [`network::block_reason`].
//!
//! It uses the WebView2 COM interface directly (`unsafe`), because wry has no API for
//! it. The decisions are all in `network.rs`, which has the tests; this file only
//! connects them to WebView2.
//!
//! wry uses the same `WebResourceRequested` event for the `mdviewer://` protocol. Both
//! handlers get every request: wry ignores addresses that are not its own, and
//! `block_reason` never blocks the page address.

use std::sync::{Arc, Mutex, PoisonError};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
    ICoreWebView2_22,
};
use webview2_com::{WebResourceRequestedEventHandler, take_pwstr};
use windows_core::{HSTRING, Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::network;
use crate::settings::Settings;

/// Starts the filter. `settings` is read for each request, so that a change of
/// "Remote images" has an effect at once.
pub fn install(webview: &WebView, settings: Arc<Mutex<Settings>>) -> windows_core::Result<()> {
    let core = webview.webview();
    let environment = webview.environment();
    let all = HSTRING::from("*");
    // SAFETY: `core` and `environment` are valid COM objects of this web view, and
    // WebView2 calls the handler on this (the UI) thread only.
    unsafe {
        // The newer interface also includes requests of workers and frames.
        match core.cast::<ICoreWebView2_22>() {
            Ok(core_22) => core_22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                &all,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?,
            Err(_) => {
                core.AddWebResourceRequestedFilter(&all, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?
            }
        }

        let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let mut uri = PWSTR::null();
            args.Request()?.Uri(&mut uri)?;
            let uri = take_pwstr(uri);

            let remote = settings
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remote_images;
            if let Some(reason) = network::block_reason(&uri, remote) {
                log::info!("request blocked ({reason}): {uri}");
                let response = environment.CreateWebResourceResponse(
                    None,
                    403,
                    &HSTRING::from("Blocked by mdviewer"),
                    &HSTRING::new(),
                )?;
                args.SetResponse(&response)?;
            }
            Ok(())
        }));
        let mut token = 0;
        core.add_WebResourceRequested(&handler, &mut token)?;
    }
    Ok(())
}
