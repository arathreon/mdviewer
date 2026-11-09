//! The viewer window: a `tao` window with a `wry` web view (WebView2 on Windows).
//!
//! The page comes from the custom protocol `mdviewer://`. The web view shows it at
//! [`platform::ORIGIN`] + the path of the document below its root (see `site.rs`); on
//! Windows, `https://mdviewer.localhost/<path>`. The system calls are in `platform`.
//!
//! Security: the Markdown file is untrusted. Documents can contain raw HTML, and
//! comrak does not remove dangerous tags. These layers protect the user:
//! - [`csp`]: no scripts, frames, plugins, forms, remote styles or remote fonts.
//! - The request filter (`network.rs`): no requests into the local network, and no
//!   requests to the internet at all when the user blocks remote images.
//! - Local files: only images and media from inside the root folder (`site.rs`).
//! - Links: the page script sends clicks to Rust, which decides (`links.rs`).
//! - Navigation, new windows, downloads and permission requests are blocked.
//! - Only the page itself can send messages to Rust (`Site::is_page_url`).
//! - Engine options (`platform`): on Windows, no automatic Windows login (NTLM) to
//!   image servers.
//! - InPrivate mode: image servers cannot store cookies.

use std::borrow::Cow;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::window::{Theme as WindowTheme, Window, WindowBuilder};
use wry::http::{Response, StatusCode, header};
use wry::{
    DragDropEvent, NewWindowResponse, PermissionResponse, RGBA, WebContext, WebView, WebViewBuilder,
};

use crate::dialect::choices::{DialectChoice, FolderChoices};
use crate::history::History;
use crate::links::{self, LinkAction};
use crate::page::{Page, first_markdown_file};
use crate::settings::{
    Choice, HistoryPanel, PanelWidth, RemoteImages, Settings, ThemeChoice, WindowState,
};
use crate::{assets, paths, platform, site, wiki};

/// The name of the custom protocol.
pub(crate) const PROTOCOL: &str = "mdviewer";

/// Content Security Policy of the page.
///
/// Documents cannot run scripts, load frames or plugins, send forms, or use their own
/// fonts. Scripts can come only from the viewer path `/_mdviewer/`. Images can come
/// from the page origin (local files, see `site.rs`) and from `data:` URLs. With
/// [`RemoteImages::Show`], images and media can also come from the internet over HTTPS
/// (for example badges in a README); the request filter still blocks local addresses.
pub fn csp(remote: RemoteImages) -> String {
    let internet = match remote {
        RemoteImages::Show => " https:",
        RemoteImages::Block => "",
    };
    // Scripts only from the viewer files (see `assets.rs`), never from the document.
    let scripts = format!("{}{}", site::ORIGIN, assets::PREFIX);
    format!(
        "default-src 'none'; \
         script-src {scripts}; \
         img-src 'self'{internet} data:; \
         media-src 'self'{internet}; \
         style-src 'self' 'unsafe-inline'; \
         font-src 'self'; \
         base-uri 'none'; \
         form-action 'none'"
    )
}

/// The script of the viewer. See `assets/viewer.js`.
const SCRIPT: &str = include_str!("../assets/viewer.js");

/// The minimum window size, in logical pixels.
const MIN_SIZE: LogicalSize<f64> = LogicalSize::new(400.0, 300.0);

/// The window size when the screen size is not known.
const FALLBACK_SIZE: LogicalSize<f64> = LogicalSize::new(1000.0, 800.0);

/// Messages from the page script. The IPC handler sends them to the event loop.
#[derive(Debug, PartialEq, Eq)]
pub enum Message {
    /// The page is shown.
    Ready,
    /// The user wants to close the window (Escape).
    Close,
    /// The user selected a theme.
    Theme(ThemeChoice),
    /// The user selected whether remote images load.
    Images(RemoteImages),
    /// The user wants to open another file ("Open…" button or Ctrl+O).
    Open,
    /// The user dropped a Markdown file on the window. Not a page message: it comes
    /// from the drag-and-drop handler.
    OpenPath(PathBuf),
    /// The content script could not render something (already cleaned for the log).
    PageError(String),
    /// The user clicked a link (the absolute URL; see `links.rs`).
    Link(String),
    /// The user clicked a wiki link that was not found when the page opened (the target,
    /// for example `Note#Heading`). Only now is the vault searched.
    WikiLink(String),
    /// Go back to the previous file (the "Back" button, Alt+Left or the mouse button).
    Back,
    /// Go forward again (the "Forward" button, Alt+Right or the mouse button).
    Forward,
    /// The user clicked a node of the history panel (its node id, see `history.rs`).
    History(usize),
    /// The vertical scroll position of the page, in CSS pixels (for "Back").
    Scroll(u32),
    /// The user opened or closed the history panel.
    Panel(HistoryPanel),
    /// The user changed the width of the history panel (already moved into its range).
    PanelWidth(PanelWidth),
    /// The user chose a dialect in the "Dialect" selector, for the folder of the current file.
    Dialect(DialectChoice),
}

/// What the viewer reads at the start and saves when the user changes it.
#[derive(Debug)]
pub struct Saved {
    /// The settings of the toolbar and the window.
    pub settings: Settings,
    /// Where `settings` is saved (`None`: not saved).
    pub settings_path: Option<PathBuf>,
    /// The dialect that the user chose for each folder ("Dialect" in the toolbar).
    pub dialects: FolderChoices,
    /// Where `dialects` is saved (`None`: not saved).
    pub dialects_path: Option<PathBuf>,
}

/// Opens the window, shows the page and waits until the user closes the window.
///
/// `started` is the start time of the program, for the time measurement in the log.
pub fn run(page: Page, saved: Saved, started: Instant) -> Result<(), Box<dyn Error>> {
    let Saved {
        settings,
        settings_path,
        mut dialects,
        dialects_path,
    } = saved;
    // The page builder, the request filter and the event loop all read the settings.
    let settings = Arc::new(Mutex::new(settings));
    let initial = lock(&settings).clone();
    let title = page.title.clone();
    let first_url = page.site.page_url();
    // The page shows the back and forward buttons and the history panel, so the page
    // handler and the event loop share the history too.
    let history = Arc::new(Mutex::new(History::new(page.path.as_deref())));
    // "Open…" and links replace the page, so the handlers and the event loop share it.
    let page = Arc::new(Mutex::new(page));
    let mut event_loop = EventLoopBuilder::<Message>::with_user_event().build();
    let size = event_loop
        .primary_monitor()
        .map(|monitor| initial_size(monitor.size().to_logical(monitor.scale_factor())))
        .unwrap_or(FALLBACK_SIZE);
    let window = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(size)
        .with_min_inner_size(MIN_SIZE)
        .with_theme(window_theme(initial.theme))
        // Hidden until it is at its place (see `place_window`), so that it does not jump.
        .with_visible(false)
        .build(&event_loop)?;
    // First the place, then maximize: "Restore down" then gives the centered window.
    place_window(&window);
    if initial.window == WindowState::Maximized {
        window.set_maximized(true);
    }
    window.set_visible(true);

    let data_dir = paths::local_dir_from_env().map(paths::webview_data_dir);
    if data_dir.is_none() {
        log::warn!("LOCALAPPDATA is not available; WebView2 uses its default data folder");
    }
    let mut web_context = WebContext::new(data_dir);

    let proxy = event_loop.create_proxy();
    let drop_proxy = event_loop.create_proxy();
    let page_settings = Arc::clone(&settings);
    let shown_page = Arc::clone(&page);
    let sender_page = Arc::clone(&page);
    let navigation_page = Arc::clone(&page);
    let shown_history = Arc::clone(&history);

    let builder =
        platform::configure_webview(WebViewBuilder::new_with_web_context(&mut web_context));
    let webview = builder
        .with_incognito(true)
        .with_background_color(background_color(window.theme()))
        // The page is built for each request, so that a reload shows the current settings.
        .with_custom_protocol(PROTOCOL.to_owned(), move |_, request| {
            respond(
                request.uri().path(),
                &lock(&shown_page),
                &lock(&page_settings),
                &lock(&shown_history),
            )
        })
        .with_initialization_script(SCRIPT)
        .with_ipc_handler(move |request| {
            let sender = request.uri().to_string();
            if !lock(&sender_page).site.is_page_url(&sender) {
                log::warn!("message from {sender} ignored");
                return;
            }
            match parse_message(request.body()) {
                Some(message) => {
                    log::debug!("message from the page: {message:?}");
                    // The send fails only when the event loop has stopped. Then nothing is to do.
                    let _ = proxy.send_event(message);
                }
                None => log::warn!("unknown message from the page: {:?}", request.body()),
            }
        })
        // Links are clicks that the page script sends to Rust, so a navigation is
        // allowed only to the page itself (a <meta refresh> is blocked here).
        .with_navigation_handler(move |url| {
            let allowed = lock(&navigation_page).site.is_page_url(&url);
            if !allowed {
                log::info!("navigation blocked: {url}");
            }
            allowed
        })
        .with_drag_drop_handler(move |event| {
            if let DragDropEvent::Drop { paths, .. } = event {
                match first_markdown_file(&paths) {
                    Some(path) => {
                        let _ = drop_proxy.send_event(Message::OpenPath(path.to_path_buf()));
                    }
                    None => log::info!("drop ignored, no Markdown file: {paths:?}"),
                }
            }
            // wry ignores this value on Windows; the drop never reaches the page.
            true
        })
        .with_new_window_req_handler(|url, _| {
            log::info!("new window blocked: {url}");
            NewWindowResponse::Deny
        })
        .with_download_started_handler(|url, _| {
            log::warn!("download blocked: {url}");
            false
        })
        .with_permission_handler(|kind| {
            log::warn!("permission request denied: {kind:?}");
            PermissionResponse::Deny
        })
        .with_url(first_url)
        .build(&window)?;

    // Without the filter, documents could send requests into the local network.
    // Thus, an error here stops the viewer (fail closed).
    platform::install_request_filter(&webview, Arc::clone(&settings))?;

    log::debug!("window created {:.0?} after start", started.elapsed());

    let mut first_page = true;
    let exit_code = event_loop.run_return(|event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Message::Ready) => {
                if first_page {
                    log::info!("page shown {:.0?} after start", started.elapsed());
                    first_page = false;
                } else {
                    log::info!("page reloaded");
                }
                // wry gives the focus to the web view before the page exists. Without this
                // second call, the keyboard (Page Down, Escape) works only after a click.
                if let Err(error) = webview.focus() {
                    log::warn!("cannot give the focus to the page: {error}");
                }
            }
            Event::UserEvent(Message::Theme(theme)) => {
                // The page script has already changed the page. Only the title bar is left.
                window.set_theme(window_theme(theme));
                let mut current = lock(&settings);
                current.theme = theme;
                save(&current, settings_path.as_deref());
            }
            Event::UserEvent(Message::Images(remote_images)) => {
                {
                    let mut current = lock(&settings);
                    current.remote_images = remote_images;
                    save(&current, settings_path.as_deref());
                }
                // The CSP is in the response header, so only a reload changes it.
                // The lock is released first: the reload asks for the page again.
                if let Err(error) = webview.reload() {
                    log::warn!("cannot reload the page: {error}");
                }
            }
            Event::UserEvent(Message::Open) => {
                let folder = lock(&page)
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf);
                match platform::pick_markdown_file(&window, folder.as_deref()) {
                    Ok(Some(path)) => {
                        start_again(&path, &history, &window, &webview, &page, &dialects)
                    }
                    Ok(None) => log::debug!("open dialog cancelled"),
                    Err(error) => log::error!("cannot show the open dialog: {error}"),
                }
            }
            Event::UserEvent(Message::OpenPath(path)) => {
                start_again(&path, &history, &window, &webview, &page, &dialects);
            }
            Event::UserEvent(Message::Link(address)) => {
                let action = links::classify(&address, &lock(&page).site);
                log::info!("link {address}: {action:?}");
                match action {
                    // Only files that the viewer shows go into the history. Web links
                    // and files that open in File Explorer do not.
                    LinkAction::OpenMarkdown { file, fragment } => follow(
                        &file,
                        fragment.as_deref(),
                        &history,
                        &window,
                        &webview,
                        &page,
                        &dialects,
                    ),
                    LinkAction::Reveal(path) => {
                        if let Err(error) = platform::reveal(&path) {
                            log::warn!("cannot show {} in Explorer: {error}", path.display());
                        }
                    }
                    LinkAction::External(address) => {
                        if let Err(error) = platform::open_external(&address) {
                            log::warn!("{error}");
                        }
                    }
                    LinkAction::Missing(path) => {
                        let name = path.file_name().map_or_else(
                            || path.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        notice(&webview, &format!("File not found: {name}"));
                    }
                    LinkAction::Blocked(reason) => {
                        notice(&webview, &format!("Link blocked: {reason}"))
                    }
                }
            }
            Event::UserEvent(Message::WikiLink(target)) => {
                let (vault, document) = {
                    let current = lock(&page);
                    (current.vault.clone(), current.path.clone())
                };
                let started = Instant::now();
                let found = document
                    .as_deref()
                    .and_then(|document| wiki::search(vault.as_deref(), document, &target));
                log::info!(
                    "wiki link search for {target:?} in {:.0?}: {found:?}",
                    started.elapsed()
                );
                match found {
                    Some(found) if crate::page::is_markdown_file(&found.file) => follow(
                        &found.file,
                        found.fragment.as_deref(),
                        &history,
                        &window,
                        &webview,
                        &page,
                        &dialects,
                    ),
                    Some(found) => {
                        if let Err(error) = platform::reveal(&found.file) {
                            log::warn!("cannot show {} in Explorer: {error}", found.file.display());
                        }
                    }
                    None => notice(&webview, &format!("Note not found: {target}")),
                }
            }
            // Back, forward and a click in the panel add no node. The page gets the
            // scroll position of the node (`data-scroll`) and the page script goes there.
            Event::UserEvent(Message::Back) => {
                let target = lock(&history).back();
                match target {
                    Some(path) => open_file(&path, None, &window, &webview, &page, &dialects),
                    None => log::debug!("back: no earlier file"),
                }
            }
            Event::UserEvent(Message::Forward) => {
                let target = lock(&history).forward();
                match target {
                    Some(path) => open_file(&path, None, &window, &webview, &page, &dialects),
                    None => log::debug!("forward: no later file"),
                }
            }
            Event::UserEvent(Message::History(id)) => {
                let target = lock(&history).go_to(id);
                match target {
                    Some(path) => open_file(&path, None, &window, &webview, &page, &dialects),
                    None => log::warn!("history: no node {id}"),
                }
            }
            Event::UserEvent(Message::Scroll(scroll)) => lock(&history).set_scroll(scroll),
            Event::UserEvent(Message::Panel(history_panel)) => {
                // The page script has already opened or closed the panel.
                let mut current = lock(&settings);
                current.history_panel = history_panel;
                save(&current, settings_path.as_deref());
            }
            Event::UserEvent(Message::PanelWidth(width)) => {
                // The page script has already changed the width; it sends the width only
                // when the drag stops.
                let mut current = lock(&settings);
                current.history_panel_width = width;
                save(&current, settings_path.as_deref());
            }
            Event::UserEvent(Message::Dialect(choice)) => change_dialect(
                choice,
                &mut dialects,
                dialects_path.as_deref(),
                &webview,
                &page,
            ),
            Event::UserEvent(Message::PageError(text)) => log::warn!("page: {text}"),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(Message::Close) => {
                remember_window_state(&window, &settings, settings_path.as_deref());
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
    log::debug!("event loop finished with code {exit_code}");
    Ok(())
}

/// The window (title bar) theme for a choice. `None` follows Windows.
///
/// The page theme does not use the WebView2 `PreferredColorScheme` setting: it had
/// no effect in InPrivate mode (tested 2026-11-06). The page uses `data-theme` instead.
pub fn window_theme(theme: ThemeChoice) -> Option<WindowTheme> {
    match theme {
        ThemeChoice::Auto => None,
        ThemeChoice::Light => Some(WindowTheme::Light),
        ThemeChoice::Dark => Some(WindowTheme::Dark),
    }
}

/// Shows another file in the window, at the heading `fragment` if it is given.
fn open_file(
    path: &Path,
    fragment: Option<&str>,
    window: &Window,
    webview: &WebView,
    page: &Mutex<Page>,
    dialects: &FolderChoices,
) {
    log::info!("open {}", path.display());
    let new_page = Page::from_file(path, dialects);
    window.set_title(&new_page.title);
    let mut url = new_page.site.page_url();
    if let Some(fragment) = fragment {
        url.push('#');
        url.extend(utf8_percent_encode(fragment, FRAGMENT));
    }
    // The page must be replaced before the load: the navigation check reads it.
    *lock(page) = new_page;
    // A new load (not a reload), so that the new page starts at the top (or the heading).
    if let Err(error) = webview.load_url(&url) {
        log::warn!("cannot show the new page: {error}");
    }
}

/// Saves the dialect `choice` for the folder of the current file (see
/// `dialect::choices`), then shows the file again with that dialect.
///
/// The URL stays the same, so a reload is sufficient. The page script sent the scroll
/// position before the message, so the page goes back to the same place. The history
/// does not change: it is the same file.
fn change_dialect(
    choice: DialectChoice,
    dialects: &mut FolderChoices,
    dialects_path: Option<&Path>,
    webview: &WebView,
    page: &Mutex<Page>,
) {
    let file = lock(page).path.clone();
    let Some(folder) = file.as_deref().and_then(Path::parent) else {
        log::warn!("dialect {choice:?} ignored: the page shows no file");
        return;
    };
    if !dialects.set(folder, choice) {
        log::warn!("cannot save a dialect for {}", folder.display());
        return;
    }
    log::info!("dialect {choice:?} chosen for {}", folder.display());
    if let Some(path) = dialects_path
        && let Err(error) = dialects.save(path)
    {
        // The choice still applies until the window closes.
        log::warn!("cannot save {}: {error}", path.display());
    }
    if let Some(file) = file {
        let new_page = Page::from_file(&file, dialects);
        // The lock is released before the reload: the reload asks for the page again.
        *lock(page) = new_page;
    }
    if let Err(error) = webview.reload() {
        log::warn!("cannot reload the page: {error}");
    }
}

/// Characters that are encoded in a URL fragment.
const FRAGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#');

/// A link to a Markdown file: the history follows the link (a new child, or a node that
/// is already in the tree, see `history.rs`), then the window shows the file. The
/// history comes first, because the new page shows it.
fn follow(
    path: &Path,
    fragment: Option<&str>,
    history: &Mutex<History>,
    window: &Window,
    webview: &WebView,
    page: &Mutex<Page>,
    dialects: &FolderChoices,
) {
    lock(history).follow_link(path, fragment.is_some());
    open_file(path, fragment, window, webview, page, dialects);
}

/// A file from "Open…" or drag and drop: the history starts again with only this file
/// (the old history is removed), then the window shows the file.
fn start_again(
    path: &Path,
    history: &Mutex<History>,
    window: &Window,
    webview: &WebView,
    page: &Mutex<Page>,
    dialects: &FolderChoices,
) {
    lock(history).start_again(path);
    open_file(path, None, window, webview, page, dialects);
}

/// Shows a short message at the bottom of the page.
fn notice(webview: &WebView, text: &str) {
    log::info!("notice: {text}");
    let script = format!(
        "window.mdviewerNotice && window.mdviewerNotice({});",
        js_string(text)
    );
    if let Err(error) = webview.evaluate_script(&script) {
        log::warn!("cannot show the notice: {error}");
    }
}

/// A JavaScript string literal for `text`. Every character that could end the string or
/// the line is escaped, so that text from a document cannot become code.
pub fn js_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003c"),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Locks shared data. A panic in another thread does not make the data unusable.
fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Saves the settings, if a settings file is known. Errors go to the log only.
fn save(settings: &Settings, path: Option<&Path>) {
    let Some(path) = path else {
        return;
    };
    match settings.save(path) {
        Ok(()) => log::info!(
            "settings saved: theme {}, remote images {}, history panel {} ({} px)",
            settings.theme.id(),
            settings.remote_images.id(),
            settings.history_panel.id(),
            settings.history_panel_width.pixels()
        ),
        Err(error) => log::warn!("cannot save {}: {error}", path.display()),
    }
}

/// Answers a request to the custom protocol, in this order:
/// 1. the page itself (its path follows the document, see `site.rs`);
/// 2. a viewer file (`/_mdviewer/...`, see [`assets`]). This path is reserved: it never
///    serves a file from the document folder, because the CSP allows scripts from it;
/// 3. a local image or media file below the root folder (`Site::servable_file`).
pub fn respond(
    path: &str,
    page: &Page,
    settings: &Settings,
    history: &History,
) -> Response<Cow<'static, [u8]>> {
    const HTML: &str = "text/html; charset=utf-8";
    // A reload must get the page with the current settings, never an old copy.
    const NO_STORE: &str = "no-store";
    let not_found = (
        StatusCode::NOT_FOUND,
        HTML,
        NO_STORE,
        Cow::Borrowed(&[][..]),
    );
    let (status, content_type, cache, body): (_, _, _, Cow<'static, [u8]>) =
        if page.site.is_page_path(path) {
            let html = page.document(settings, history).into_bytes();
            (StatusCode::OK, HTML, NO_STORE, Cow::Owned(html))
        } else if path.starts_with(assets::PREFIX) {
            match assets::find(path) {
                Some(asset) => {
                    log::debug!("served {path} ({} bytes)", asset.content.len());
                    // Embedded files change only with a new program version.
                    let cache = "max-age=31536000, immutable";
                    (
                        StatusCode::OK,
                        asset.content_type,
                        cache,
                        Cow::Borrowed(asset.content),
                    )
                }
                None => {
                    log::info!("not found: {path}");
                    not_found
                }
            }
        } else {
            let file = page.site.servable_file(path).and_then(|file| {
                let content = std::fs::read(&file).map_err(|_| "cannot read the file")?;
                let content_type = site::media_type(&file).ok_or("file type is not allowed")?;
                Ok((content, content_type))
            });
            match file {
                Ok((content, content_type)) => {
                    log::debug!("served {path} ({} bytes)", content.len());
                    (StatusCode::OK, content_type, NO_STORE, Cow::Owned(content))
                }
                Err(reason) => {
                    log::info!("not served ({reason}): {path}");
                    not_found
                }
            }
        };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_SECURITY_POLICY, csp(settings.remote_images))
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        // Image servers do not learn that the request comes from mdviewer.
        .header(header::REFERRER_POLICY, "no-referrer")
        .header(header::CACHE_CONTROL, cache)
        .body(body)
        .expect("response parts are valid")
}

/// Makes text from the page safe for one log line: no control characters (a document
/// could otherwise add fake log lines through a diagram error), and at most 300 characters.
pub fn clean_log_text(text: &str) -> String {
    const MAX_CHARS: usize = 300;
    let mut clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_CHARS)
        .collect();
    if text.chars().nth(MAX_CHARS).is_some() {
        clean.push('…');
    }
    clean
}

/// Reads a message from the page script.
pub fn parse_message(text: &str) -> Option<Message> {
    match text {
        "ready" => Some(Message::Ready),
        "close" => Some(Message::Close),
        "open" => Some(Message::Open),
        "back" => Some(Message::Back),
        "forward" => Some(Message::Forward),
        _ => {
            let (kind, value) = text.split_once(':')?;
            match kind {
                "theme" => ThemeChoice::from_id(value).map(Message::Theme),
                "images" => RemoteImages::from_id(value).map(Message::Images),
                "panel" => HistoryPanel::from_id(value).map(Message::Panel),
                "panelwidth" => PanelWidth::parse(value).map(Message::PanelWidth),
                "dialect" => DialectChoice::from_id(value).map(Message::Dialect),
                "history" => value.parse().ok().map(Message::History),
                "scroll" => value.parse().ok().map(Message::Scroll),
                "error" => Some(Message::PageError(clean_log_text(value))),
                "link" => Some(Message::Link(value.to_owned())),
                "wikilink" => Some(Message::WikiLink(value.to_owned())),
                _ => None,
            }
        }
    }
}

/// The first window size: 85 % of the screen height, and not wider than the page
/// needs (or 90 % of the screen width on small screens).
pub fn initial_size(screen: LogicalSize<f64>) -> LogicalSize<f64> {
    const PAGE_WIDTH: f64 = 1100.0;
    LogicalSize::new(
        PAGE_WIDTH
            .min(screen.width * 90.0 / 100.0)
            .max(MIN_SIZE.width),
        (screen.height * 85.0 / 100.0).max(MIN_SIZE.height),
    )
}

/// Saves whether the window is maximized, when the window closes, so that the next
/// window opens the same way. The file is written only if the state changed.
fn remember_window_state(window: &Window, settings: &Mutex<Settings>, path: Option<&Path>) {
    let mut current = lock(settings);
    let state =
        WindowState::when_closed(current.window, window.is_maximized(), window.is_minimized());
    if state != current.window {
        current.window = state;
        save(&current, path);
    }
}

/// Puts the window at the center of the work area (the screen without the taskbar).
///
/// Without this, Windows chooses the position, and the bottom of a tall window can be
/// behind the taskbar (seen on a 1920 x 1080 screen: 40 pixels). The status bar of the
/// page (the link preview) is at the bottom, so it must be visible.
fn place_window(window: &Window) {
    match platform::work_area() {
        Some(area) => {
            let size = window.outer_size();
            let (x, y) = centered_position(area, (size.width, size.height));
            window.set_outer_position(PhysicalPosition::new(x, y));
        }
        None => log::warn!("work area not known; Windows chooses the window position"),
    }
}

/// The top left corner of a window of `size` (width, height) at the center of `area`
/// (left, top, right, bottom). All values are physical pixels. A window that is larger
/// than the area starts at the top left corner of the area, so that its title bar is
/// visible.
pub fn centered_position(area: (i32, i32, i32, i32), size: (u32, u32)) -> (i32, i32) {
    let (left, top, right, bottom) = area;
    let free = |room: i32, length: u32| (room - i32::try_from(length).unwrap_or(i32::MAX)).max(0);
    (
        left + free(right - left, size.0) / 2,
        top + free(bottom - top, size.1) / 2,
    )
}

/// The background color of the web view before the page shows. It agrees with the
/// page colors, so that the window does not flash white in the dark theme.
pub fn background_color(theme: WindowTheme) -> RGBA {
    match theme {
        WindowTheme::Dark => (0x0d, 0x11, 0x17, 0xff),
        _ => (0xff, 0xff, 0xff, 0xff),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::Site;

    fn page() -> Page {
        Page {
            title: "t".to_owned(),
            body: "<p>page body</p>\n".to_owned(),
            path: None,
            vault: None,
            site: Site::none(),
            dialect: None,
        }
    }

    fn images(remote_images: RemoteImages) -> Settings {
        Settings {
            remote_images,
            ..Settings::default()
        }
    }

    #[test]
    fn root_gives_page_with_security_headers() {
        let settings = Settings::default();
        let response = respond("/", &page(), &settings, &History::default());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.body().as_ref(),
            page().document(&settings, &History::default()).as_bytes()
        );
        let headers = response.headers();
        assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(
            headers[header::CONTENT_SECURITY_POLICY],
            csp(RemoteImages::Show).as_str()
        );
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn viewer_files_are_served_with_their_type_and_the_csp() {
        let response = respond(
            "/_mdviewer/katex/katex.min.js",
            &page(),
            &Settings::default(),
            &History::default(),
        );
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(
            headers[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "max-age=31536000, immutable"
        );
        assert!(headers.contains_key(header::CONTENT_SECURITY_POLICY));
        assert_eq!(
            response.body().as_ref(),
            assets::find("/_mdviewer/katex/katex.min.js")
                .unwrap()
                .content
        );
    }

    #[test]
    fn page_errors_are_cleaned_for_the_log() {
        assert_eq!(
            parse_message("error:Diagram error: bad\ntoken"),
            Some(Message::PageError("Diagram error: bad token".to_owned()))
        );
        assert_eq!(clean_log_text("a\r\nb\tc\u{1b}[31m"), "a  b c [31m");
        let long = "x".repeat(400);
        let cleaned = clean_log_text(&long);
        assert_eq!(cleaned.chars().count(), 301);
        assert!(cleaned.ends_with('…'));
        assert_eq!(clean_log_text(&"é".repeat(300)), "é".repeat(300));
    }

    #[test]
    fn page_and_csp_follow_the_current_settings() {
        let blocked = images(RemoteImages::Block);
        let response = respond("/", &page(), &blocked, &History::default());
        assert_eq!(
            response.headers()[header::CONTENT_SECURITY_POLICY],
            csp(RemoteImages::Block).as_str()
        );
        let html = String::from_utf8(response.body().to_vec()).unwrap();
        assert!(html.contains("<option value=\"block\" selected>"), "{html}");
    }

    #[test]
    fn other_paths_are_not_found_and_also_have_the_csp() {
        for path in ["/other.md", "/image.png", "", "/../secret"] {
            let response = respond(path, &page(), &Settings::default(), &History::default());
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            assert!(response.body().is_empty(), "{path}");
            assert_eq!(
                response.headers()[header::CONTENT_SECURITY_POLICY],
                csp(RemoteImages::Show).as_str()
            );
        }
    }

    /// Splits a CSP into (directive, sources) pairs.
    fn csp_directives(remote: RemoteImages) -> Vec<(String, Vec<String>)> {
        csp(remote)
            .split(';')
            .map(|part| {
                let mut words = part.split_whitespace().map(str::to_owned);
                (words.next().unwrap(), words.collect())
            })
            .collect()
    }

    fn csp_sources(remote: RemoteImages, directive: &str) -> Vec<String> {
        csp_directives(remote)
            .into_iter()
            .find(|(name, _)| name == directive)
            .map(|(_, sources)| sources)
            .unwrap_or_else(|| panic!("{directive} missing"))
    }

    #[test]
    fn csp_blocks_scripts_frames_plugins_and_forms() {
        for &remote in RemoteImages::ALL {
            assert_eq!(csp_sources(remote, "default-src"), ["'none'"]);
            assert_eq!(csp_sources(remote, "base-uri"), ["'none'"]);
            assert_eq!(csp_sources(remote, "form-action"), ["'none'"]);
            // Scripts: only the viewer path, no 'self', no inline code, no eval.
            assert_eq!(
                csp_sources(remote, "script-src"),
                ["https://mdviewer.localhost/_mdviewer/"]
            );
            // These fall back to default-src 'none' (or script-src) only if they are not set.
            for directive in [
                "script-src-elem",
                "script-src-attr",
                "frame-src",
                "child-src",
                "object-src",
                "connect-src",
                "worker-src",
            ] {
                assert!(
                    csp_directives(remote)
                        .iter()
                        .all(|(name, _)| name != directive),
                    "{directive}"
                );
            }
            assert!(!csp(remote).contains("unsafe-eval"));
        }
    }

    #[test]
    fn csp_allows_remote_content_only_over_https() {
        for &remote in RemoteImages::ALL {
            for (directive, sources) in csp_directives(remote) {
                for source in sources {
                    assert!(
                        matches!(
                            source.as_str(),
                            "'none'"
                                | "'self'"
                                | "'unsafe-inline'"
                                | "https:"
                                | "data:"
                                | "https://mdviewer.localhost/_mdviewer/"
                        ),
                        "{directive} {source}"
                    );
                    // 'unsafe-inline' is accepted for styles only (Mermaid SVGs need it).
                    if source == "'unsafe-inline'" {
                        assert_eq!(directive, "style-src");
                    }
                }
            }
            // Remote styles and document fonts can send page text to a server together
            // with remote images (CSS and font ligature attacks), so they are not allowed.
            assert!(!csp_sources(remote, "style-src").contains(&"https:".to_owned()));
            assert_eq!(csp_sources(remote, "font-src"), ["'self'"]);
            // `data:` media could hide large content; only images need it.
            assert!(!csp_sources(remote, "media-src").contains(&"data:".to_owned()));
        }
    }

    #[test]
    fn remote_images_setting_controls_https_in_the_csp() {
        let https = "https:".to_owned();
        assert!(csp_sources(RemoteImages::Show, "img-src").contains(&https));
        assert!(csp_sources(RemoteImages::Show, "media-src").contains(&https));
        // "Block": no directive allows the internet (`https:` as a whole scheme).
        for (directive, sources) in csp_directives(RemoteImages::Block) {
            assert!(!sources.contains(&https), "{directive}");
        }
        assert_eq!(
            csp_sources(RemoteImages::Block, "img-src"),
            ["'self'", "data:"]
        );
    }

    /// A page in a temporary vault with an image, and a file below `_mdviewer/`.
    fn page_in_vault() -> (crate::test_util::TempDir, Page) {
        let dir = crate::test_util::TempDir::new("viewer-files");
        let root = dir.path().join("Vault");
        std::fs::create_dir_all(root.join("img")).unwrap();
        std::fs::create_dir_all(root.join("_mdviewer")).unwrap();
        std::fs::write(root.join("img/a.png"), b"PNG data").unwrap();
        std::fs::write(root.join("_mdviewer/render.png"), b"fake").unwrap();
        std::fs::write(root.join("note.md"), "# Note\n").unwrap();
        let page = Page {
            site: Site::for_document(&root, &root.join("note.md")),
            ..page()
        };
        (dir, page)
    }

    #[test]
    fn local_images_are_served_with_their_type() {
        let (_dir, page) = page_in_vault();
        let response = respond(
            "/img/a.png",
            &page,
            &Settings::default(),
            &History::default(),
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(response.body().as_ref(), b"PNG data");
        let page_response = respond("/note.md", &page, &Settings::default(), &History::default());
        assert_eq!(
            page_response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
    }

    #[test]
    fn the_viewer_path_never_serves_document_files() {
        // The CSP allows scripts from /_mdviewer/, so documents must not add files there.
        let (_dir, page) = page_in_vault();
        let response = respond(
            "/_mdviewer/render.png",
            &page,
            &Settings::default(),
            &History::default(),
        );
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn history_messages_are_parsed() {
        assert_eq!(parse_message("forward"), Some(Message::Forward));
        assert_eq!(parse_message("history:3"), Some(Message::History(3)));
        assert_eq!(parse_message("scroll:1250"), Some(Message::Scroll(1250)));
        assert_eq!(
            parse_message("panel:show"),
            Some(Message::Panel(HistoryPanel::Show))
        );
        assert_eq!(
            parse_message("panel:hide"),
            Some(Message::Panel(HistoryPanel::Hide))
        );
        // The width is moved into its range; only whole numbers are accepted.
        assert_eq!(
            parse_message("panelwidth:340"),
            Some(Message::PanelWidth(PanelWidth::new(340)))
        );
        assert_eq!(
            parse_message("panelwidth:5"),
            Some(Message::PanelWidth(PanelWidth::new(PanelWidth::MIN)))
        );
        for bad in [
            "panelwidth:",
            "panelwidth:wide",
            "panelwidth:-1",
            "history:",
            "history:-1",
            "history:x",
            "scroll:-5",
            "scroll:1.5",
            "scroll:99999999999",
            "panel:open",
            "Forward",
        ] {
            assert_eq!(parse_message(bad), None, "{bad}");
        }
    }

    #[test]
    fn page_shows_the_history() {
        let (_dir, page) = page_in_vault();
        let mut history = History::new(Some(Path::new(r"D:\note.md")));
        history.set_scroll(77);
        let response = respond("/note.md", &page, &Settings::default(), &history);
        let html = String::from_utf8(response.body().to_vec()).unwrap();
        assert!(html.contains("data-scroll=\"77\""), "{html}");
        assert!(html.contains("data-history=\"0\""), "{html}");
    }

    #[test]
    fn link_messages_are_parsed() {
        assert_eq!(parse_message("back"), Some(Message::Back));
        assert_eq!(
            parse_message("link:https://example.com/a:b"),
            Some(Message::Link("https://example.com/a:b".to_owned()))
        );
        assert_eq!(
            parse_message("wikilink:Note#Part"),
            Some(Message::WikiLink("Note#Part".to_owned()))
        );
    }

    #[test]
    fn js_string_cannot_end_the_string_or_the_line() {
        assert_eq!(js_string("plain"), r#""plain""#);
        // The backslash comes from a variable, so that no tool can change the escapes.
        let b = '\\';
        let line_separator = char::from_u32(0x2028).unwrap();
        assert_eq!(
            js_string(&format!("a\"b{b}c\nd{line_separator}e<x")),
            format!("\"a{b}\"b{b}{b}c{b}u000ad{b}u2028e{b}u003cx\"")
        );
    }

    #[test]
    fn messages_are_parsed_exactly() {
        assert_eq!(parse_message("ready"), Some(Message::Ready));
        assert_eq!(parse_message("close"), Some(Message::Close));
        assert_eq!(parse_message("open"), Some(Message::Open));
        assert_eq!(
            parse_message("theme:dark"),
            Some(Message::Theme(ThemeChoice::Dark))
        );
        assert_eq!(
            parse_message("theme:auto"),
            Some(Message::Theme(ThemeChoice::Auto))
        );
        assert_eq!(
            parse_message("images:block"),
            Some(Message::Images(RemoteImages::Block))
        );
        assert_eq!(
            parse_message("images:show"),
            Some(Message::Images(RemoteImages::Show))
        );
        assert_eq!(parse_message("theme:purple"), None);
        assert_eq!(parse_message("theme:"), None);
        assert_eq!(parse_message("images:dark"), None);
        assert_eq!(
            parse_message("dialect:auto"),
            Some(Message::Dialect(DialectChoice::Auto))
        );
        assert_eq!(
            parse_message("dialect:obsidian"),
            DialectChoice::from_id("obsidian").map(Message::Dialect)
        );
        assert!(parse_message("dialect:github").is_some());
        assert_eq!(parse_message("dialect:Obsidian"), None);
        assert_eq!(parse_message("dialect:"), None);
        assert_eq!(parse_message("colors:dark"), None);
        assert_eq!(parse_message("Close"), None);
        assert_eq!(parse_message(""), None);
    }

    #[test]
    fn theme_choices_map_to_window_themes() {
        assert_eq!(window_theme(ThemeChoice::Auto), None);
        assert_eq!(window_theme(ThemeChoice::Light), Some(WindowTheme::Light));
        assert_eq!(window_theme(ThemeChoice::Dark), Some(WindowTheme::Dark));
    }

    #[test]
    fn initial_size_on_large_and_small_screens() {
        assert_eq!(
            initial_size(LogicalSize::new(2560.0, 1440.0)),
            LogicalSize::new(1100.0, 1224.0)
        );
        // 1920 x 1080 at 150 %.
        assert_eq!(
            initial_size(LogicalSize::new(1280.0, 720.0)),
            LogicalSize::new(1100.0, 612.0)
        );
        assert_eq!(
            initial_size(LogicalSize::new(1000.0, 600.0)),
            LogicalSize::new(900.0, 510.0)
        );
        assert_eq!(initial_size(LogicalSize::new(100.0, 100.0)), MIN_SIZE);
    }

    #[test]
    fn window_is_centered_in_the_work_area() {
        // 1920 x 1080 with a 48-pixel taskbar at the bottom; a 1116 x 958 window.
        assert_eq!(
            centered_position((0, 0, 1920, 1032), (1116, 958)),
            (402, 37)
        );
        // A taskbar at the top or the left moves the area.
        assert_eq!(
            centered_position((60, 40, 1980, 1120), (1000, 1000)),
            (520, 80)
        );
        // A second screen at the left of the primary one has negative coordinates.
        assert_eq!(
            centered_position((-1920, 0, 0, 1080), (920, 80)),
            (-1420, 500)
        );
        // Larger than the area: the title bar stays visible at the top left corner.
        assert_eq!(centered_position((0, 0, 800, 600), (1000, 700)), (0, 0));
        assert_eq!(
            centered_position((0, 0, 800, 600), (u32::MAX, u32::MAX)),
            (0, 0)
        );
    }

    #[test]
    fn background_agrees_with_page_colors() {
        let css = include_str!("../assets/style.css");
        assert!(css.contains("--bg: light-dark(#ffffff, #0d1117);"));
        assert_eq!(
            background_color(WindowTheme::Dark),
            (0x0d, 0x11, 0x17, 0xff)
        );
        assert_eq!(
            background_color(WindowTheme::Light),
            (0xff, 0xff, 0xff, 0xff)
        );
    }

    #[test]
    fn script_runs_only_on_the_page_origin() {
        assert!(SCRIPT.contains(&format!("location.origin !== \"{}\"", site::ORIGIN)));
    }

    #[test]
    fn script_uses_the_select_ids_of_the_page() {
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::THEME_SELECT_ID)));
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::IMAGES_SELECT_ID)));
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::DIALECT_SELECT_ID)));
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::OPEN_BUTTON_ID)));
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::BACK_BUTTON_ID)));
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::FORWARD_BUTTON_ID)));
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::PANEL_BUTTON_ID)));
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::PANEL_ID)));
        assert!(SCRIPT.contains(&format!("\"{}\"", crate::page::RESIZER_ID)));
        // The script uses the same limits for the width as the settings.
        for (name, value) in [
            ("PANEL_MIN", PanelWidth::MIN),
            ("PANEL_MAX", PanelWidth::MAX),
            ("PANEL_DEFAULT", PanelWidth::DEFAULT),
        ] {
            assert!(SCRIPT.contains(&format!("{name} = {value};")), "{name}");
        }
    }
}
