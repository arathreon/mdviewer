//! Obsidian Flavored Markdown, as Obsidian shows notes in Reading view.

use super::{Dialect, Rule, Tier};
use crate::context::Context;
use crate::feature::{
    Autolinks, Callouts, Comments, Feature, Footnotes, FrontMatter, HardBreaks, HeadingAnchors,
    Highlight, InlineFootnotes, MathDollars, RawHtml, Strikethrough, Tables, TaskLists, WikiLinks,
};

/// The Obsidian dialect.
pub const DIALECT: Dialect = Dialect {
    id: "obsidian",
    name: "Obsidian",
    rules: RULES,
    features,
};

/// The detection rules: the two vault signals (strong) and Obsidian-only syntax (weak).
/// Content rules read [`Context::prose`], so that examples in code do not count.
const RULES: &[Rule] = &[
    Rule {
        tier: Tier::Location,
        weight: 100,
        reason: ".obsidian folder found (Obsidian vault)",
        test: |ctx| ctx.vault_root().is_some(),
    },
    Rule {
        tier: Tier::Location,
        weight: 100,
        reason: "vault listed in Obsidian (obsidian.json)",
        test: |ctx| ctx.known_vault().is_some(),
    },
    Rule {
        tier: Tier::Content,
        weight: 10,
        reason: "wiki links found",
        test: |ctx| has_wiki_link(ctx.prose()),
    },
    Rule {
        tier: Tier::Content,
        weight: 10,
        reason: "Obsidian callout found",
        test: |ctx| has_obsidian_callout(ctx.prose()),
    },
    Rule {
        tier: Tier::Content,
        weight: 5,
        reason: "highlight (==text==) found",
        test: |ctx| has_highlight(ctx.prose()),
    },
    Rule {
        tier: Tier::Content,
        weight: 5,
        reason: "comment (%%text%%) found",
        test: |ctx| has_comment(ctx.prose()),
    },
];

/// The syntax features of Obsidian. Soft line breaks show as line breaks, unless the
/// vault has the setting "Strict line breaks".
fn features(ctx: &Context) -> Vec<&'static dyn Feature> {
    let mut features: Vec<&'static dyn Feature> = vec![
        &RawHtml,
        &Tables,
        &Strikethrough,
        &Autolinks,
        &TaskLists,
        &Footnotes,
        &InlineFootnotes,
        &HeadingAnchors,
        &FrontMatter,
        &Callouts,
        &MathDollars,
        &WikiLinks,
        &Highlight,
        &Comments,
    ];
    if !ctx.strict_line_breaks() {
        features.push(&HardBreaks);
    }
    features
}

/// `[[target]]` on one line.
fn has_wiki_link(prose: &str) -> bool {
    prose.lines().any(|line| {
        line.find("[[")
            .is_some_and(|start| line[start + 2..].find("]]").is_some_and(|len| len > 0))
    })
}

/// `==text==` on one line. The text must not start or end with a space, so that
/// `a == b` and setext heading lines (`=====`) do not match.
fn has_highlight(prose: &str) -> bool {
    prose.lines().any(|line| {
        let mut rest = line;
        while let Some(start) = rest.find("==") {
            let after = &rest[start + 2..];
            let opens = after.starts_with(|c: char| !c.is_whitespace() && c != '=');
            if opens
                && after
                    .find("==")
                    .is_some_and(|end| !after[..end].ends_with(char::is_whitespace))
            {
                return true;
            }
            rest = after;
        }
        false
    })
}

/// Two or more `%%` markers: an inline comment or a block comment.
fn has_comment(prose: &str) -> bool {
    prose.matches("%%").nth(1).is_some()
}

/// A callout that GitHub does not support: an Obsidian-only type, a title, or a fold marker.
fn has_obsidian_callout(prose: &str) -> bool {
    const GITHUB_TYPES: &[&str] = &["note", "tip", "important", "warning", "caution"];
    prose.lines().any(|line| {
        let Some(quoted) = line.trim_start().strip_prefix('>') else {
            return false;
        };
        let Some(marker) = quoted.trim_start().strip_prefix("[!") else {
            return false;
        };
        let Some((kind, after)) = marker.split_once(']') else {
            return false;
        };
        let github_type = GITHUB_TYPES.iter().any(|t| kind.eq_ignore_ascii_case(t));
        !github_type || !after.trim().is_empty()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn html(markdown: &str) -> String {
        render(markdown, &features(&Context::detached("note.md", markdown)))
    }

    #[test]
    fn wiki_link_detection() {
        assert!(has_wiki_link("see [[Note]]\n"));
        assert!(has_wiki_link("![[image.png]]\n"));
        assert!(!has_wiki_link("[[]]\n"));
        assert!(!has_wiki_link("[[open\nclose]]\n"));
        assert!(!has_wiki_link("[link](url)\n"));
    }

    #[test]
    fn highlight_detection() {
        assert!(has_highlight("a ==marked== b\n"));
        assert!(!has_highlight("Title\n=====\n"));
        assert!(!has_highlight("if a == b == c\n"));
        assert!(!has_highlight("a ==open\nclose== b\n"));
    }

    #[test]
    fn comment_detection() {
        assert!(has_comment("text %%hidden%% text\n"));
        assert!(has_comment("%%\nblock\n%%\n"));
        assert!(!has_comment("100%% sure\n"));
    }

    #[test]
    fn callout_detection_ignores_plain_github_alerts() {
        assert!(has_obsidian_callout("> [!faq] Question\n"));
        assert!(has_obsidian_callout("> [!abstract]\n"));
        assert!(has_obsidian_callout("> [!note]- Folded\n"));
        assert!(has_obsidian_callout("> [!NOTE] Custom title\n"));
        assert!(!has_obsidian_callout("> [!NOTE]\n"));
        assert!(!has_obsidian_callout("> [!warning]\n"));
        assert!(!has_obsidian_callout("> quote\n"));
    }

    #[test]
    fn soft_line_break_is_a_line_break_by_default() {
        assert!(html("one\ntwo\n").contains("one<br />"));
    }

    #[test]
    fn strict_line_breaks_setting_removes_hard_breaks() {
        let dir = crate::test_util::TempDir::new("obsidian-strict");
        std::fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        std::fs::write(
            dir.path().join(".obsidian/app.json"),
            r#"{"strictLineBreaks": true}"#,
        )
        .unwrap();
        let ctx = Context::with_ceiling(
            dir.path().join("note.md"),
            "one\ntwo\n".to_owned(),
            Some(dir.path()),
        );
        assert_eq!(render(ctx.text(), &features(&ctx)), "<p>one\ntwo</p>\n");
    }

    #[test]
    fn wiki_links_with_alias() {
        let out = html("[[Some Note|alias]]\n");
        assert!(out.contains("data-wikilink"), "{out}");
        assert!(out.contains(">alias</a>"), "{out}");
    }

    #[test]
    fn highlight_and_comments() {
        let out = html("==marked== visible %%hidden%%\n");
        assert!(out.contains("<mark>marked</mark>"), "{out}");
        assert!(out.contains("visible"), "{out}");
        assert!(!out.contains("hidden"), "{out}");
    }

    #[test]
    fn callouts_include_github_alert_types_titles_and_folding() {
        let github_style = html("> [!NOTE]\n> Body\n");
        assert!(
            github_style.contains(r#"data-callout="note""#),
            "{github_style}"
        );

        let titled = html("> [!faq]- Why?\n> Because.\n");
        assert!(titled.contains(r#"data-callout="faq""#), "{titled}");
        assert!(titled.contains("Why?"), "{titled}");
        assert!(titled.contains(r#"data-callout-fold="#), "{titled}");
    }

    #[test]
    fn inline_footnotes() {
        assert!(html("Text^[inline note].\n").contains("footnote-ref"));
    }

    #[test]
    fn github_only_syntax_is_not_active() {
        let out = html(":rocket: $`x`$\n");
        assert!(out.contains(":rocket:"), "{out}");
        assert!(!out.contains(r#"<code data-math-style"#), "{out}");
    }
}
