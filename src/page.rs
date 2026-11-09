//! The HTML page that the viewer shows.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::context::Context;
use crate::dialect::choices::{DialectChoice, FolderChoices, Origin, Selection};
use crate::history::History;
use crate::render::RenderEnv;
use crate::settings::{Choice, HistoryPanel, PanelWidth, Settings};
use crate::site::Site;
use crate::{assets, dialect, render};

/// The style sheet of the page, embedded in the program.
const STYLE: &str = include_str!("../assets/style.css");

/// The name of the program in window titles. (Folders and the log use "mdviewer".)
pub const APP_NAME: &str = "MDViewer";

/// The id of the theme selector. The page script uses it too.
pub const THEME_SELECT_ID: &str = "mdviewer-theme";

/// The id of the "Remote images" selector. The page script uses it too.
pub const IMAGES_SELECT_ID: &str = "mdviewer-images";

/// The id of the "Dialect" selector. The page script uses it too.
pub const DIALECT_SELECT_ID: &str = "mdviewer-dialect";

/// The id of the "Open…" button. The page script uses it too.
pub const OPEN_BUTTON_ID: &str = "mdviewer-open";

/// The id of the "Back" button. The page script uses it too.
pub const BACK_BUTTON_ID: &str = "mdviewer-back";

/// The id of the "Forward" button. The page script uses it too.
pub const FORWARD_BUTTON_ID: &str = "mdviewer-forward";

/// The id of the button that opens and closes the history panel. The page script uses it too.
pub const PANEL_BUTTON_ID: &str = "mdviewer-panel-button";

/// The id of the history panel. The page script uses it too.
pub const PANEL_ID: &str = "mdviewer-history";

/// The id of the line that changes the width of the history panel. The page script
/// uses it too.
pub const RESIZER_ID: &str = "mdviewer-resizer";

/// File name extensions of Markdown files (for the "Open" dialog and for drag and drop).
pub const MARKDOWN_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdown", "mkd", "mkdn", "mdwn", "mdtxt", "mdtext",
];

/// Gives `true` if the file name has a Markdown extension (in any case).
pub fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            MARKDOWN_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// The first Markdown file of a list, for example of the files dropped on the window.
pub fn first_markdown_file(paths: &[PathBuf]) -> Option<&Path> {
    paths
        .iter()
        .map(PathBuf::as_path)
        .find(|path| is_markdown_file(path))
}

/// The content of the viewer window.
#[derive(Debug)]
pub struct Page {
    /// The window title.
    pub title: String,
    /// The HTML fragment inside the page (without `<html>` and `<body>`).
    pub body: String,
    /// The absolute path of the Markdown file, if the page shows a file (or tried to).
    pub path: Option<PathBuf>,
    /// The Obsidian vault of the file, for the search of wiki links on click.
    pub vault: Option<PathBuf>,
    /// The page URL and the local files that the page can load.
    pub site: Site,
    /// The detected and the chosen dialect, for the "Dialect" selector. `None` for
    /// message and error pages (they have no Markdown).
    pub dialect: Option<Selection>,
}

impl Page {
    /// Reads a Markdown file, detects its dialect and makes the page.
    ///
    /// If the user chose a dialect for the folder of the file (or a parent folder) in
    /// `choices`, that dialect renders the page. The detection runs also then, so that
    /// the selector can show the detected dialect.
    ///
    /// If the file cannot be read, the page shows the error.
    pub fn from_file(path: &Path, choices: &FolderChoices) -> Page {
        Self::from_loaded(path, Context::load(path), choices)
    }

    /// Makes the page of [`Page::from_file`] from the result of a [`Context`] load.
    ///
    /// Tests give a context from [`Context::load_in`], so that the folders and the vault
    /// list of the computer cannot change the page.
    fn from_loaded(path: &Path, loaded: std::io::Result<Context>, choices: &FolderChoices) -> Page {
        let started = Instant::now();
        let ctx = match loaded {
            Ok(ctx) => ctx,
            Err(error) => {
                log::error!("cannot read {}: {error}", path.display());
                return Page {
                    // The folder can exist although the file does not; "Open…" starts there.
                    path: std::path::absolute(path).ok(),
                    ..Page::message(
                        &title_for(&file_name_of(path)),
                        "Cannot open the file",
                        &format!("{}\n{error}", path.display()),
                    )
                };
            }
        };
        let selection = Selection {
            detection: dialect::detect(&ctx),
            saved: ctx
                .path()
                .parent()
                .and_then(|folder| choices.for_folder(folder)),
        };
        let features = (selection.dialect().features)(&ctx);
        let site = Site::for_document(&site_root(&ctx), ctx.path());
        let env = RenderEnv {
            document: Some(ctx.path()),
            vault: ctx.obsidian_vault(),
            site: Some(&site),
        };
        let body = render::render_in(ctx.text(), &features, &env);
        log::info!(
            "{}: dialect {}, {} bytes of HTML in {:.2?}",
            ctx.path().display(),
            selection.describe(),
            body.len(),
            started.elapsed()
        );
        Page {
            title: title_for(ctx.file_name().unwrap_or("?")),
            body,
            path: Some(ctx.path().to_path_buf()),
            vault: ctx.obsidian_vault().map(Path::to_path_buf),
            site,
            dialect: Some(selection),
        }
    }

    /// A page with a heading and a short text, for example for errors.
    pub fn message(title: &str, heading: &str, text: &str) -> Page {
        Page {
            title: title.to_owned(),
            path: None,
            vault: None,
            site: Site::none(),
            dialect: None,
            body: format!(
                "<div class=\"mdviewer-message\">\n<h1>{}</h1>\n<p>{}</p>\n</div>\n",
                escape(heading),
                escape(text).replace('\n', "<br>\n")
            ),
        }
    }

    /// The full HTML document of the page, with the buttons and the panel for `history`.
    pub fn document(&self, settings: &Settings, history: &History) -> String {
        document(
            &self.title,
            &self.body,
            settings,
            history,
            self.dialect.as_ref(),
        )
    }
}

/// The root folder of a document's site: the outer one of its vault and its Git
/// repository (one is always inside the other, because both contain the document), or
/// the document folder. The page can load images only from below this folder.
pub fn site_root(ctx: &Context) -> PathBuf {
    [ctx.obsidian_vault(), ctx.repo_root()]
        .into_iter()
        .flatten()
        .min_by_key(|folder| folder.components().count())
        .or_else(|| ctx.path().parent())
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// The window title for a file name, for example `"MDViewer - README.md"`. Without a
/// file, the title is only [`APP_NAME`].
pub fn title_for(file_name: &str) -> String {
    format!("{APP_NAME} - {file_name}")
}

/// The name of a file for its title, or the whole path if it has no file name.
fn file_name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Puts an HTML fragment into a full HTML document with the style sheet, the toolbar,
/// the back and forward buttons and the history panel.
///
/// The viewer elements come before the document content. Thus, `getElementById` finds
/// them first, even if the document has elements with the same ids.
///
/// The `<html>` element always has all its attributes: the HTML parser ignores the
/// attributes of a second `<html>` tag (from the document) that are already set.
/// `data-scroll` is the scroll position of the history entry (for "Back"). The `style`
/// attribute gives the CSS the width of the history panel (`--panel-width`).
pub fn document(
    title: &str,
    body: &str,
    settings: &Settings,
    history: &History,
    dialect: Option<&Selection>,
) -> String {
    format!(
        "<!DOCTYPE html>\n\
         <html data-theme=\"{}\" data-panel=\"{}\" data-scroll=\"{}\" style=\"--panel-width: {}px\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <link rel=\"icon\" href=\"data:,\">\n\
         <title>{}</title>\n\
         <style>\n{STYLE}</style>\n\
         <script type=\"module\" src=\"{}{}\"></script>\n\
         </head>\n\
         <body>\n\
         {}{}{}{}\
         <article class=\"markdown-body\">\n{body}</article>\n\
         </body>\n\
         </html>\n",
        settings.theme.id(),
        settings.history_panel.id(),
        history.current().map_or(0, |entry| entry.scroll),
        settings.history_panel_width.pixels(),
        escape(title),
        assets::PREFIX,
        assets::RENDER_SCRIPT,
        navigation(settings, history),
        history_panel(history),
        resizer(settings),
        toolbar(settings, dialect)
    )
}

/// The line at the right side of the history panel. The user drags it to change the
/// width of the panel, or uses the arrow keys when it has the keyboard focus. A
/// double-click gives the default width. The page script does the work.
fn resizer(settings: &Settings) -> String {
    format!(
        "<div id=\"{RESIZER_ID}\" class=\"mdviewer-resizer\" role=\"separator\" \
         aria-orientation=\"vertical\" aria-controls=\"{PANEL_ID}\" \
         aria-label=\"Width of the history panel\" aria-valuemin=\"{}\" aria-valuemax=\"{}\" \
         aria-valuenow=\"{}\" tabindex=\"0\" \
         title=\"Drag to change the width (double-click: default width)\"></div>\n",
        PanelWidth::MIN,
        PanelWidth::MAX,
        settings.history_panel_width.pixels()
    )
}

/// The buttons at the top left: "Back", "Forward" and the history panel button.
/// The icons come from the style sheet; the text for the user is in `aria-label`.
fn navigation(settings: &Settings, history: &History) -> String {
    let disabled = |possible: bool| if possible { "" } else { " disabled" };
    format!(
        "<div class=\"mdviewer-nav\">\n\
         <button type=\"button\" id=\"{BACK_BUTTON_ID}\" title=\"Back (Alt+Left)\" aria-label=\"Back\"{}></button>\n\
         <button type=\"button\" id=\"{FORWARD_BUTTON_ID}\" title=\"Forward (Alt+Right)\" aria-label=\"Forward\"{}></button>\n\
         <button type=\"button\" id=\"{PANEL_BUTTON_ID}\" title=\"Files of this session (Ctrl+H)\" \
         aria-label=\"History\" aria-controls=\"{PANEL_ID}\" aria-pressed=\"{}\"></button>\n\
         </div>\n",
        disabled(history.can_go_back()),
        disabled(history.can_go_forward()),
        settings.history_panel == HistoryPanel::Show
    )
}

/// The deepest indentation level of the history panel. Deeper nodes stay at this level,
/// so that a long chain of links does not push the names out of the narrow panel.
pub const MAX_PANEL_DEPTH: usize = 8;

/// The smallest width in CSS pixels that the history panel leaves for the page. The
/// document has a maximum width of 980 px with 40 px padding on each side, so 600 px
/// keep about 520 px of text: about 65 characters, a line length that is easy to read.
/// The style sheet and the page script use the same number (a test checks it).
pub const MIN_PAGE_WIDTH: u32 = 600;

/// The history panel: the tree of the visits of this session (see `history.rs`), in the
/// order of the visits. The CSS indents each line by its `data-depth`, fades a file
/// that is on an earlier line too, and shows the panel only when
/// `<html data-panel="show">`.
///
/// Privacy: the file paths are only in the text, never in attributes (for example a
/// `title` tooltip). CSS in a document can test attribute values with selectors such
/// as `[title^="C:\Users\a"]` and load a remote image for each match. Thus, it could
/// send parts of a path (the user name) to a server. CSS cannot test text this way.
/// A click sends the node id, not the path.
fn history_panel(history: &History) -> String {
    let mut html = format!(
        "<nav id=\"{PANEL_ID}\" class=\"mdviewer-history\" aria-label=\"Files of this session\">\n\
         <h2>History</h2>\n"
    );
    let rows = history.rows();
    if rows.is_empty() {
        html.push_str("<p class=\"mdviewer-history-empty\">No files yet.</p>\n");
    } else {
        html.push_str("<ol>\n");
        for row in rows {
            let path = row.path;
            let name = path
                .file_name()
                .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy());
            let folder = path.parent().map(Path::to_string_lossy).unwrap_or_default();
            html.push_str(&format!(
                "<li data-depth=\"{}\"{}><button type=\"button\" data-history=\"{}\"{}>\
                 <span class=\"mdviewer-history-name\">{}</span>\
                 <span class=\"mdviewer-history-folder\">{}</span></button></li>\n",
                row.depth.min(MAX_PANEL_DEPTH),
                if row.repeated {
                    " class=\"mdviewer-history-repeated\""
                } else {
                    ""
                },
                row.id,
                if row.current {
                    " aria-current=\"page\""
                } else {
                    ""
                },
                escape(&name),
                escape(&folder)
            ));
        }
        html.push_str("</ol>\n");
    }
    html.push_str("</nav>\n");
    html
}

/// The toolbar at the top right: the "Open…" button, the "Dialect" selector (only for
/// a Markdown file) and the selectors for the settings.
fn toolbar(settings: &Settings, dialect: Option<&Selection>) -> String {
    format!(
        "<div class=\"mdviewer-toolbar\">\n\
         <button type=\"button\" id=\"{OPEN_BUTTON_ID}\" title=\"Open a Markdown file (Ctrl+O)\">Open…</button>\n\
         {}{}{}</div>\n",
        dialect.map(dialect_select).unwrap_or_default(),
        select(THEME_SELECT_ID, "Color theme", "Theme", settings.theme),
        select(
            IMAGES_SELECT_ID,
            "Images from the internet",
            "Remote images",
            settings.remote_images
        )
    )
}

/// The "Dialect" selector: "Auto (<detected dialect>)", then each known dialect.
///
/// The tooltip tells the detected dialect with its reasons, and whether the choice is
/// saved for this folder or a parent folder. It never contains a path: CSS in a
/// document can read attribute values (see [`history_panel`]).
fn dialect_select(selection: &Selection) -> String {
    let detected = selection.detection.dialect.name;
    let mut description = format!(
        "Markdown dialect. A choice is saved for this folder and its subfolders. \
         Detected: {detected} ({}).",
        selection.detection.reasons.join(", ")
    );
    if let Some((DialectChoice::Fixed(_), origin)) = selection.saved {
        description.push_str(match origin {
            Origin::ThisFolder => " Chosen for this folder.",
            Origin::ParentFolder => " Chosen for a parent folder.",
        });
    }
    let mut html = format!(
        "<select id=\"{DIALECT_SELECT_ID}\" title=\"{}\" aria-label=\"Markdown dialect\">\n",
        escape(&description)
    );
    for choice in DialectChoice::all() {
        let selected = if choice == selection.choice() {
            " selected"
        } else {
            ""
        };
        let name = match choice {
            DialectChoice::Auto => format!("Auto ({detected})"),
            DialectChoice::Fixed(dialect) => dialect.name.to_owned(),
        };
        html.push_str(&format!(
            "<option value=\"{}\"{selected}>Dialect: {}</option>\n",
            choice.id(),
            escape(&name)
        ));
    }
    html.push_str("</select>\n");
    html
}

/// A selector for one setting. Each option shows as `"<label>: <name>"`.
fn select<C: Choice>(id: &str, description: &str, label: &str, current: C) -> String {
    let mut html =
        format!("<select id=\"{id}\" title=\"{description}\" aria-label=\"{description}\">\n");
    for &choice in C::ALL {
        let selected = if choice == current { " selected" } else { "" };
        html.push_str(&format!(
            "<option value=\"{}\"{selected}>{label}: {}</option>\n",
            choice.id(),
            choice.name()
        ));
    }
    html.push_str("</select>\n");
    html
}

/// Replaces the characters that have a special meaning in HTML.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{RemoteImages, ThemeChoice};
    use crate::test_util::TempDir;

    /// Makes the page of `path` as [`Page::from_file`] does, but the folder search stops
    /// at the test folder `dir` and Obsidian's vault list is empty. Thus, a `.git` or
    /// `.obsidian` folder above the temporary folder, or the vaults of the user, cannot
    /// change the result.
    fn page_in(dir: &TempDir, path: &Path, choices: &FolderChoices) -> Page {
        Page::from_loaded(path, Context::load_in(path, Some(dir.path()), &[]), choices)
    }

    fn with_theme(theme: ThemeChoice) -> Settings {
        Settings {
            theme,
            ..Settings::default()
        }
    }

    #[test]
    fn markdown_files_are_recognized_by_extension_in_any_case() {
        for name in [
            "a.md",
            "a.MD",
            "notes.Markdown",
            "x.mkd",
            "dir.with.dots/a.mdown",
        ] {
            assert!(is_markdown_file(Path::new(name)), "{name}");
        }
        for name in ["a.txt", "a.png", "md", "a.md.zip", "a", ".md"] {
            assert!(!is_markdown_file(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn first_markdown_file_skips_other_files() {
        let paths: Vec<PathBuf> = ["image.png", "b.md", "c.md"].map(PathBuf::from).into();
        assert_eq!(first_markdown_file(&paths), Some(Path::new("b.md")));
        assert_eq!(first_markdown_file(&[PathBuf::from("a.txt")]), None);
        assert_eq!(first_markdown_file(&[]), None);
    }

    #[test]
    fn escape_replaces_special_characters_only() {
        assert_eq!(
            escape(r#"<a href="x">Tom & 'Jerry'</a> ž"#),
            "&lt;a href=&quot;x&quot;&gt;Tom &amp; &#39;Jerry&#39;&lt;/a&gt; ž"
        );
    }

    #[test]
    fn document_contains_escaped_title_style_and_body() {
        let html = document(
            "a < b",
            "<p>Body</p>\n",
            &Settings::default(),
            &History::default(),
            None,
        );
        assert!(html.starts_with("<!DOCTYPE html>\n"), "{html}");
        assert!(html.contains("<title>a &lt; b</title>"), "{html}");
        assert!(html.contains(".markdown-body {"), "{html}");
        // An empty icon, so that WebView2 does not ask for /favicon.ico.
        assert!(
            html.contains(r#"<link rel="icon" href="data:,">"#),
            "{html}"
        );
        // The content script, from the path that the CSP allows.
        let script = r#"<script type="module" src="/_mdviewer/render.mjs"></script>"#;
        assert!(html.contains(script), "{html}");
        assert!(assets::find("/_mdviewer/render.mjs").is_some());
        assert!(
            html.contains("<article class=\"markdown-body\">\n<p>Body</p>\n</article>"),
            "{html}"
        );
    }

    #[test]
    fn toolbar_comes_before_the_content() {
        let body = "<button id=\"mdviewer-open\"></button>\
                    <select id=\"mdviewer-theme\"></select><select id=\"mdviewer-images\"></select>";
        let html = document("t", body, &Settings::default(), &History::default(), None);
        let toolbar = html.find("class=\"mdviewer-toolbar\"").unwrap();
        let article = html.find("<article").unwrap();
        assert!(toolbar < article, "{html}");
        for id in [
            OPEN_BUTTON_ID,
            THEME_SELECT_ID,
            IMAGES_SELECT_ID,
            BACK_BUTTON_ID,
            FORWARD_BUTTON_ID,
            PANEL_BUTTON_ID,
            PANEL_ID,
            RESIZER_ID,
        ] {
            assert!(html.find(id).unwrap() < article, "{id}");
        }
    }

    /// A history that starts at the first path and follows links to the other paths.
    fn history_of(paths: &[&str]) -> History {
        let mut history = History::default();
        for path in paths {
            history.follow_link(Path::new(path), false);
        }
        history
    }

    #[test]
    fn back_and_forward_buttons_are_disabled_when_not_possible() {
        let back = format!("id=\"{BACK_BUTTON_ID}\" title=\"Back (Alt+Left)\" aria-label=\"Back\"");
        let forward = format!(
            "id=\"{FORWARD_BUTTON_ID}\" title=\"Forward (Alt+Right)\" aria-label=\"Forward\""
        );
        let settings = Settings::default();

        let html = document("t", "", &settings, &History::default(), None);
        assert!(html.contains(&format!("{back} disabled>")), "{html}");
        assert!(html.contains(&format!("{forward} disabled>")), "{html}");

        let mut history = history_of(&[r"D:\a.md", r"D:\b.md"]);
        let html = document("t", "", &settings, &history, None);
        assert!(html.contains(&format!("{back}>")), "{html}");
        assert!(html.contains(&format!("{forward} disabled>")), "{html}");

        history.back();
        let html = document("t", "", &settings, &history, None);
        assert!(html.contains(&format!("{back} disabled>")), "{html}");
        assert!(html.contains(&format!("{forward}>")), "{html}");
    }

    #[test]
    fn panel_shows_the_tree_and_marks_the_current_node() {
        let mut history = history_of(&[r"D:\Notes\a.md", r"D:\Notes\Sub\<b>.md"]);
        history.back();
        let html = document("t", "", &Settings::default(), &history, None);
        // In the order of the visits; data-history is the node id.
        let first = "<li data-depth=\"0\"><button type=\"button\" data-history=\"0\" aria-current=\"page\">\
                     <span class=\"mdviewer-history-name\">a.md</span>\
                     <span class=\"mdviewer-history-folder\">D:\\Notes</span></button></li>";
        let second = "<li data-depth=\"1\"><button type=\"button\" data-history=\"1\">\
                      <span class=\"mdviewer-history-name\">&lt;b&gt;.md</span>\
                      <span class=\"mdviewer-history-folder\">D:\\Notes\\Sub</span></button></li>";
        assert!(html.contains(first), "{html}");
        assert!(html.contains(second), "{html}");
        assert_eq!(html.matches(" aria-current=").count(), 1, "{html}");
    }

    #[test]
    fn panel_fades_repeated_files_and_limits_the_indentation() {
        // a → b, back to a, a → c → b: the second b is a repeated file.
        let mut history = history_of(&[r"D:\a.md", r"D:\b.md"]);
        history.back();
        history.follow_link(Path::new(r"D:\c.md"), false);
        history.follow_link(Path::new(r"D:\b.md"), false);
        let html = document("t", "", &Settings::default(), &history, None);
        assert!(
            html.contains(
                "<li data-depth=\"2\" class=\"mdviewer-history-repeated\">\
                 <button type=\"button\" data-history=\"3\" aria-current=\"page\">"
            ),
            "{html}"
        );
        assert_eq!(html.matches("mdviewer-history-repeated\"").count(), 1);

        let names: Vec<String> = (0..12).map(|i| format!(r"D:\n{i}.md")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let html = document("t", "", &Settings::default(), &history_of(&names), None);
        assert!(html.contains("<li data-depth=\"8\">"), "{html}");
        assert!(!html.contains("<li data-depth=\"9\">"), "{html}");
        // The style sheet has a rule for each level.
        for depth in 1..=MAX_PANEL_DEPTH {
            let rule = format!(".mdviewer-history li[data-depth=\"{depth}\"]");
            assert!(STYLE.contains(&rule), "{rule}");
        }
    }

    #[test]
    fn panel_has_no_paths_in_attributes() {
        // CSS in a document can test attribute values, but not text (see `history_panel`).
        let history = history_of(&[r"C:\Users\secret\Notes\a.md"]);
        let html = document("t", "", &Settings::default(), &history, None);
        let panel = &html[html.find("<nav id=").unwrap()..html.find("</nav>").unwrap()];
        for tag in panel.split('<').skip(1) {
            let attributes = tag.split_once('>').map_or(tag, |(inside, _)| inside);
            assert!(!attributes.contains("secret"), "{tag}");
        }
        assert!(
            panel.contains(">C:\\Users\\secret\\Notes</span>"),
            "{panel}"
        );
    }

    #[test]
    fn panel_without_files_says_so() {
        let html = document("t", "", &Settings::default(), &History::default(), None);
        assert!(
            html.contains("<p class=\"mdviewer-history-empty\">No files yet.</p>"),
            "{html}"
        );
    }

    #[test]
    fn html_element_has_the_panel_state_and_the_scroll_position() {
        let mut history = history_of(&[r"D:\a.md"]);
        history.set_scroll(321);
        for &history_panel in HistoryPanel::ALL {
            let settings = Settings {
                history_panel,
                ..Settings::default()
            };
            let html = document("t", "", &settings, &history, None);
            let tag = format!(
                "<html data-theme=\"auto\" data-panel=\"{}\" data-scroll=\"321\" \
                 style=\"--panel-width: 260px\">",
                history_panel.id()
            );
            assert!(html.contains(&tag), "{html}");
            let pressed = format!("aria-pressed=\"{}\"", history_panel == HistoryPanel::Show);
            assert!(html.contains(&pressed), "{html}");
            assert!(
                STYLE.contains(":root[data-panel=\"show\"]"),
                "the style sheet must show the panel"
            );
        }
        // Without a file, the page starts at the top.
        let html = document("t", "", &Settings::default(), &History::default(), None);
        assert!(html.contains("data-scroll=\"0\" "), "{html}");
    }

    #[test]
    fn panel_width_goes_to_the_css_and_to_the_resizer() {
        let settings = Settings {
            history_panel_width: PanelWidth::new(345),
            ..Settings::default()
        };
        let html = document("t", "", &settings, &History::default(), None);
        assert!(html.contains(" style=\"--panel-width: 345px\">"), "{html}");
        let resizer = format!(
            "<div id=\"{RESIZER_ID}\" class=\"mdviewer-resizer\" role=\"separator\" \
             aria-orientation=\"vertical\" aria-controls=\"{PANEL_ID}\" \
             aria-label=\"Width of the history panel\" aria-valuemin=\"160\" aria-valuemax=\"800\" \
             aria-valuenow=\"345\" tabindex=\"0\""
        );
        assert!(html.contains(&resizer), "{html}");
        // The style sheet uses the width for the panel, the page and the resizer.
        assert!(STYLE.contains("  width: var(--panel-size);"));
        assert!(STYLE.contains("padding-left: var(--panel-size);"));
        assert!(STYLE.contains("left: calc(var(--panel-size) - 3px);"));
        assert_eq!(PanelWidth::DEFAULT, 260, "the CSS fallback must agree");
    }

    #[test]
    fn the_panel_leaves_the_same_room_for_the_page_in_the_css_and_the_script() {
        let min = PanelWidth::MIN;
        let rule = format!(
            "--panel-size: clamp({min}px, var(--panel-width, {}px), calc(100vw - {MIN_PAGE_WIDTH}px));",
            PanelWidth::DEFAULT
        );
        assert!(STYLE.contains(&rule), "{rule}");
        let script = include_str!("../assets/viewer.js");
        assert!(script.contains(&format!("const PAGE_MIN = {MIN_PAGE_WIDTH};")));
    }

    #[test]
    fn toolbar_has_open_button_with_shortcut_hint() {
        let html = document("t", "", &Settings::default(), &History::default(), None);
        let button = format!(
            "<button type=\"button\" id=\"{OPEN_BUTTON_ID}\" title=\"Open a Markdown file (Ctrl+O)\">Open…</button>"
        );
        assert!(html.contains(&button), "{html}");
    }

    #[test]
    fn html_element_has_the_theme_for_the_css() {
        for &theme in ThemeChoice::ALL {
            let html = document("t", "", &with_theme(theme), &History::default(), None);
            let tag = format!("<html data-theme=\"{}\" data-panel=", theme.id());
            assert!(html.contains(&tag), "{html}");
            let rule = format!(":root[data-theme=\"{}\"]", theme.id());
            assert!(
                theme == ThemeChoice::Auto || STYLE.contains(&rule),
                "{rule}"
            );
        }
    }

    #[test]
    fn toolbar_selects_the_current_values_only() {
        for &theme in ThemeChoice::ALL {
            for &remote_images in RemoteImages::ALL {
                let settings = Settings {
                    theme,
                    remote_images,
                    ..Settings::default()
                };
                let html = document("t", "", &settings, &History::default(), None);
                assert_eq!(html.matches(" selected").count(), 2, "{html}");
                for id in [theme.id(), remote_images.id()] {
                    let option = format!("<option value=\"{id}\" selected>");
                    assert!(html.contains(&option), "{html}");
                }
            }
        }
    }

    #[test]
    fn toolbar_options_have_readable_names() {
        let html = document("t", "", &Settings::default(), &History::default(), None);
        assert!(html.contains(">Theme: Dark</option>"), "{html}");
        assert!(html.contains(">Remote images: Block</option>"), "{html}");
    }

    #[test]
    fn message_escapes_text_and_keeps_line_breaks() {
        let page = Page::message("T", "<Heading>", "line 1\n<line 2>");
        assert_eq!(page.title, "T");
        assert_eq!(page.path, None);
        assert!(
            page.body.contains("<h1>&lt;Heading&gt;</h1>"),
            "{}",
            page.body
        );
        assert!(
            page.body.contains("line 1<br>\n&lt;line 2&gt;"),
            "{}",
            page.body
        );
    }

    #[test]
    fn from_file_renders_markdown_with_file_name_as_title() {
        let dir = TempDir::new("page-file");
        let path = dir.path().join("README.md");
        std::fs::write(&path, "# Hello\n\n:rocket:\n").unwrap();
        let page = page_in(&dir, &path, &FolderChoices::default());
        assert_eq!(page.title, "MDViewer - README.md");
        assert_eq!(page.path.as_deref(), Some(path.as_path()));
        assert!(
            page.body.contains(r#"<h1 id="hello">Hello"#),
            "{}",
            page.body
        );
        assert!(page.body.contains('🚀'), "{}", page.body);
        let html = page.document(&Settings::default(), &History::default());
        assert!(
            html.contains("<title>MDViewer - README.md</title>"),
            "{html}"
        );
    }

    #[test]
    fn from_file_in_a_vault_gets_its_site_and_resolves_links() {
        let dir = TempDir::new("page-vault");
        let vault = dir.path().join("Vault");
        std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
        std::fs::create_dir_all(vault.join("Daily")).unwrap();
        std::fs::create_dir_all(vault.join("img")).unwrap();
        std::fs::write(vault.join("Other.md"), "x").unwrap();
        std::fs::write(vault.join("img/a.png"), b"png").unwrap();
        let note = vault.join("Daily/note.md");
        std::fs::write(&note, "![a](../img/a.png) [[Other]]\n").unwrap();

        let page = page_in(&dir, &note, &FolderChoices::default());
        assert_eq!(page.site.page_path(), "/Daily/note.md");
        assert_eq!(page.vault.as_deref(), Some(vault.as_path()));
        assert!(page.site.servable_file("/img/a.png").is_ok());
        assert!(
            page.body.contains(r#"<a href="/Other.md">Other</a>"#),
            "{}",
            page.body
        );
        assert!(
            page.body.contains(r#"<img src="../img/a.png""#),
            "{}",
            page.body
        );
    }

    #[test]
    fn site_root_is_the_outer_of_vault_and_repository() {
        let dir = TempDir::new("page-site-root");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("notes/.obsidian")).unwrap();
        let ctx = Context::with_ceiling(repo.join("notes/a.md"), String::new(), Some(dir.path()));
        assert_eq!(site_root(&ctx), repo);

        let loose =
            Context::with_ceiling(dir.path().join("x/a.md"), String::new(), Some(dir.path()));
        assert_eq!(site_root(&loose), dir.path().join("x"));
    }

    #[test]
    fn titles_name_the_program_first_and_then_the_file() {
        // Without a file (main.rs uses APP_NAME alone), and with a file.
        assert_eq!(APP_NAME, "MDViewer");
        assert_eq!(title_for("today.md"), "MDViewer - today.md");
        assert_eq!(
            file_name_of(Path::new(r"D:\Notes\Daily\today.md")),
            "today.md"
        );
        assert_eq!(file_name_of(Path::new(r"D:\")), r"D:\");
    }

    #[test]
    fn from_file_shows_error_page_for_missing_file() {
        let dir = TempDir::new("page-missing");
        let path = dir.path().join("missing.md");
        // The real `from_file`: a missing file causes no folder search, so the
        // computer cannot change the result.
        let page = Page::from_file(&path, &FolderChoices::default());
        // Only the file name in the title; the full path is in the page text. The text
        // is HTML, so characters such as `'` and `&` in the path are escaped.
        assert_eq!(page.title, "MDViewer - missing.md");
        assert!(
            page.body.contains(&escape(&path.display().to_string())),
            "{}",
            page.body
        );
        // Kept, so that "Open…" starts in the folder of the missing file.
        assert_eq!(page.path.as_deref(), Some(path.as_path()));
        assert!(page.body.contains("Cannot open the file"), "{}", page.body);
        assert!(page.body.contains("missing.md"), "{}", page.body);
        // An error page has no Markdown, so it has no "Dialect" selector.
        assert!(page.dialect.is_none());
    }

    /// The options of the "Dialect" selector: (value, text, selected).
    fn dialect_options(html: &str) -> Vec<(String, String, bool)> {
        let start = html
            .find(&format!("<select id=\"{DIALECT_SELECT_ID}\""))
            .expect("no dialect selector");
        let select = &html[start..start + html[start..].find("</select>").unwrap()];
        select
            .split("<option value=\"")
            .skip(1)
            .map(|option| {
                let (value, rest) = option.split_once('"').unwrap();
                let (attributes, text) = rest.split_once('>').unwrap();
                let text = text.split_once("</option>").unwrap().0;
                (
                    value.to_owned(),
                    text.to_owned(),
                    attributes.contains("selected"),
                )
            })
            .collect()
    }

    /// The `title` of the "Dialect" selector.
    fn dialect_title(html: &str) -> &str {
        let start = html
            .find(&format!("<select id=\"{DIALECT_SELECT_ID}\""))
            .unwrap();
        let title = &html[start..];
        let title = &title[title.find("title=\"").unwrap() + 7..];
        &title[..title.find('"').unwrap()]
    }

    #[test]
    fn the_detected_dialect_is_auto_and_named_in_the_selector() {
        let dir = TempDir::new("page-dialect-auto");
        let path = dir.path().join("note.md");
        std::fs::write(&path, "a ==marked== word\n").unwrap();
        let page = page_in(&dir, &path, &FolderChoices::default());
        assert!(page.body.contains("<mark>marked</mark>"), "{}", page.body);
        let html = page.document(&Settings::default(), &History::default());
        assert_eq!(
            dialect_options(&html),
            [
                ("auto".into(), "Dialect: Auto (Obsidian)".into(), true),
                ("github".into(), "Dialect: GitHub".into(), false),
                ("obsidian".into(), "Dialect: Obsidian".into(), false),
            ]
        );
        assert_eq!(
            dialect_title(&html),
            "Markdown dialect. A choice is saved for this folder and its subfolders. \
             Detected: Obsidian (highlight (==text==) found)."
        );
    }

    #[test]
    fn a_chosen_dialect_renders_the_page_and_is_selected() {
        let dir = TempDir::new("page-dialect-chosen");
        let folder = dir.path().join("Notes");
        std::fs::create_dir_all(folder.join("Daily")).unwrap();
        let path = folder.join("Daily").join("note.md");
        std::fs::write(&path, "a ==marked== word\n").unwrap();
        let mut choices = FolderChoices::default();
        choices.set(&folder, DialectChoice::from_id("github").unwrap());

        let page = page_in(&dir, &path, &choices);
        // GitHub has no highlight syntax: the text stays as it is.
        assert!(!page.body.contains("<mark>"), "{}", page.body);
        assert!(page.body.contains("==marked=="), "{}", page.body);
        let html = page.document(&Settings::default(), &History::default());
        let selected: Vec<_> = dialect_options(&html)
            .into_iter()
            .filter(|(_, _, selected)| *selected)
            .map(|(value, _, _)| value)
            .collect();
        assert_eq!(selected, ["github"]);
        // The "Auto" entry still names the detected dialect.
        assert_eq!(dialect_options(&html)[0].1, "Dialect: Auto (Obsidian)");
        let title = dialect_title(&html);
        assert!(title.ends_with(" Chosen for a parent folder."), "{title}");
        // Privacy: no path in the attribute (see `history_panel`).
        assert!(!title.contains("Notes"), "{title}");
        assert!(!html.contains(&dir.path().display().to_string()), "{html}");
    }

    #[test]
    fn message_pages_have_no_dialect_selector() {
        let page = Page::message("T", "Heading", "text");
        let html = page.document(&Settings::default(), &History::default());
        assert!(!html.contains(DIALECT_SELECT_ID), "{html}");
        // The other selectors are still there.
        assert!(html.contains(THEME_SELECT_ID), "{html}");
    }
}
