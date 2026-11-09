//! Syntax features. A dialect is a list of these.
//!
//! Each feature is a small type that sets the parser and renderer options it needs.
//! A feature can also change the parsed document before the HTML is written
//! ([`Feature::transform`]); the default does nothing, so most features ignore it.

use comrak::Options;
use comrak::nodes::AstNode;

use crate::render::RenderEnv;
use crate::wiki;

/// One syntax feature, for example tables or wiki links.
///
/// A trait (not data like [`crate::dialect::Dialect`]), because features have behavior:
/// they set options and can change the parsed document.
pub trait Feature: Sync {
    /// A short, fixed name for logs and tests, for example `"tables"`.
    fn id(&self) -> &'static str;

    /// Sets the `comrak` options for this feature.
    fn configure(&self, options: &mut Options<'_>);

    /// Changes the parsed document, for example to resolve links. Does nothing by default.
    fn transform<'a>(&self, _root: &'a AstNode<'a>, _env: &RenderEnv<'_>) {}
}

/// Raw HTML in the Markdown is copied to the output.
///
/// Security: this is safe only because the web view blocks scripts with a Content
/// Security Policy. `comrak` no longer has a tag filter.
pub struct RawHtml;
impl Feature for RawHtml {
    fn id(&self) -> &'static str {
        "raw-html"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.render.r#unsafe = true;
    }
}

/// GitHub tables.
pub struct Tables;
impl Feature for Tables {
    fn id(&self) -> &'static str {
        "tables"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.table = true;
    }
}

/// `~~strikethrough~~`.
pub struct Strikethrough;
impl Feature for Strikethrough {
    fn id(&self) -> &'static str {
        "strikethrough"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.strikethrough = true;
    }
}

/// Bare URLs such as `https://example.com` become links.
pub struct Autolinks;
impl Feature for Autolinks {
    fn id(&self) -> &'static str {
        "autolinks"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.autolink = true;
    }
}

/// `- [ ] task` and `- [x] done`, shown as disabled check boxes.
pub struct TaskLists;
impl Feature for TaskLists {
    fn id(&self) -> &'static str {
        "task-lists"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.tasklist = true;
        options.render.tasklist_classes = true;
    }
}

/// `text[^1]` with `[^1]: note`.
pub struct Footnotes;
impl Feature for Footnotes {
    fn id(&self) -> &'static str {
        "footnotes"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.footnotes = true;
    }
}

/// Obsidian inline footnotes: `text^[note]`.
pub struct InlineFootnotes;
impl Feature for InlineFootnotes {
    fn id(&self) -> &'static str {
        "inline-footnotes"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.footnotes = true;
        options.extension.inline_footnotes = true;
    }
}

/// Headings get an `id`, so that links to `#section` work.
pub struct HeadingAnchors;
impl Feature for HeadingAnchors {
    fn id(&self) -> &'static str {
        "heading-anchors"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.header_id_prefix = Some(String::new());
    }
}

/// A YAML block between `---` lines at the start of the file. It is not shown yet.
pub struct FrontMatter;
impl Feature for FrontMatter {
    fn id(&self) -> &'static str {
        "front-matter"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.front_matter_delimiter = Some("---".to_owned());
    }
}

/// GitHub alerts: `> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`, `> [!CAUTION]`.
pub struct Alerts;
impl Feature for Alerts {
    fn id(&self) -> &'static str {
        "alerts"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.alerts = true;
    }
}

/// Obsidian callouts: all GitHub alert types, more types, titles, and folding (`[!faq]-`).
pub struct Callouts;
impl Feature for Callouts {
    fn id(&self) -> &'static str {
        "callouts"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.ofm_callouts = true;
    }
}

/// Math with dollars: `$inline$` and `$$display$$`.
pub struct MathDollars;
impl Feature for MathDollars {
    fn id(&self) -> &'static str {
        "math-dollars"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.math_dollars = true;
    }
}

/// GitHub math with code syntax: `` $`inline`$ `` and ` ```math ` blocks.
pub struct MathCode;
impl Feature for MathCode {
    fn id(&self) -> &'static str {
        "math-code"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.math_code = true;
    }
}

/// Emoji shortcodes: `:rocket:`.
pub struct Emoji;
impl Feature for Emoji {
    fn id(&self) -> &'static str {
        "emoji"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.shortcodes = true;
    }
}

/// GitHub's special rules for nested emphasis, so that the output agrees with GitHub.
pub struct GithubQuirks;
impl Feature for GithubQuirks {
    fn id(&self) -> &'static str {
        "github-quirks"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.render.gfm_quirks = true;
    }
}

/// Obsidian wiki links: `[[Note]]`, `[[Note|alias]]`, `[[Note#Heading]]` and image
/// embeds (`![[image.png]]`), resolved in the vault (see `wiki.rs`). Note embeds
/// (`![[Note]]`) show as links for now.
pub struct WikiLinks;
impl Feature for WikiLinks {
    fn id(&self) -> &'static str {
        "wiki-links"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.wikilinks_title_after_pipe = true;
    }
    fn transform<'a>(&self, root: &'a AstNode<'a>, env: &RenderEnv<'_>) {
        wiki::resolve(root, env);
    }
}

/// Obsidian highlight: `==text==`.
pub struct Highlight;
impl Feature for Highlight {
    fn id(&self) -> &'static str {
        "highlight"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.highlight = true;
    }
}

/// Obsidian comments: `%%hidden%%`. They are not shown.
pub struct Comments;
impl Feature for Comments {
    fn id(&self) -> &'static str {
        "comments"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.extension.ofm_comments = true;
    }
}

/// One line break in the source becomes a line break in the output (Obsidian default).
pub struct HardBreaks;
impl Feature for HardBreaks {
    fn id(&self) -> &'static str {
        "hard-breaks"
    }
    fn configure(&self, options: &mut Options<'_>) {
        options.render.hardbreaks = true;
    }
}
