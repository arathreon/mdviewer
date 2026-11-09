//! GitHub Flavored Markdown, as GitHub shows `.md` files.

use super::{Dialect, Rule, Tier};
use crate::context::Context;
use crate::feature::{
    Alerts, Autolinks, Emoji, Feature, Footnotes, FrontMatter, GithubQuirks, HeadingAnchors,
    MathCode, MathDollars, RawHtml, Strikethrough, Tables, TaskLists,
};

/// The GitHub dialect. It is the first entry of `DIALECTS`, so it is the default.
pub const DIALECT: Dialect = Dialect {
    id: "github",
    name: "GitHub",
    rules: RULES,
    features,
};

/// The detection rules: a Git repository (strong) and a typical file name (weak).
const RULES: &[Rule] = &[
    Rule {
        tier: Tier::Location,
        weight: 50,
        reason: "Git repository found",
        test: |ctx| ctx.repo_root().is_some(),
    },
    Rule {
        tier: Tier::Content,
        weight: 10,
        reason: "typical repository file name",
        test: |ctx| ctx.file_name().is_some_and(is_repository_file_name),
    },
];

/// The syntax features of GitHub. They are the same for every document.
fn features(_: &Context) -> Vec<&'static dyn Feature> {
    vec![
        &RawHtml,
        &Tables,
        &Strikethrough,
        &Autolinks,
        &TaskLists,
        &Footnotes,
        &HeadingAnchors,
        &FrontMatter,
        &Alerts,
        &MathDollars,
        &MathCode,
        &Emoji,
        &GithubQuirks,
    ]
}

/// File names that are typical for repositories, for example `README.md`.
fn is_repository_file_name(file_name: &str) -> bool {
    const STEMS: &[&str] = &[
        "README",
        "CHANGELOG",
        "CONTRIBUTING",
        "LICENSE",
        "SECURITY",
        "CODE_OF_CONDUCT",
    ];
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    let is_markdown =
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown");
    is_markdown && STEMS.iter().any(|known| stem.eq_ignore_ascii_case(known))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn html(markdown: &str) -> String {
        render(markdown, &features(&Context::detached("test.md", markdown)))
    }

    #[test]
    fn repository_file_names_are_recognized_in_any_case() {
        assert!(is_repository_file_name("README.md"));
        assert!(is_repository_file_name("readme.markdown"));
        assert!(is_repository_file_name("Code_Of_Conduct.MD"));
        assert!(!is_repository_file_name("README.txt"));
        assert!(!is_repository_file_name("README"));
        assert!(!is_repository_file_name("notes.md"));
    }

    #[test]
    fn soft_line_break_is_not_a_line_break() {
        assert_eq!(html("one\ntwo\n"), "<p>one\ntwo</p>\n");
    }

    #[test]
    fn tables_strikethrough_autolinks_and_task_lists() {
        let out = html("| a |\n|---|\n| 1 |\n\n~~old~~ https://example.com\n\n- [x] done\n");
        assert!(out.contains("<table>"), "{out}");
        assert!(out.contains("<del>old</del>"), "{out}");
        assert!(out.contains(r#"<a href="https://example.com">"#), "{out}");
        assert!(out.contains(r#"class="task-list-item-checkbox""#), "{out}");
        assert!(out.contains("checked"), "{out}");
    }

    #[test]
    fn footnotes_and_heading_anchors() {
        let out = html("# My Title\n\nText[^1].\n\n[^1]: Note.\n");
        assert!(out.contains(r#"id="my-title""#), "{out}");
        assert!(out.contains("footnote-ref"), "{out}");
    }

    #[test]
    fn alerts() {
        let out = html("> [!WARNING]\n> Careful.\n");
        assert!(out.contains("markdown-alert-warning"), "{out}");
    }

    #[test]
    fn math_with_dollars_and_with_code_syntax() {
        let out = html("$a$ and $`b`$\n\n```math\nc\n```\n");
        assert!(
            out.contains(r#"<span data-math-style="inline">a</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<code data-math-style="inline">b</code>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"class="language-math" data-math-style="display""#),
            "{out}"
        );
    }

    #[test]
    fn math_blocks_keep_backslashes() {
        let out = html("```math\na \\\\ b\n```\n\n$$\nc \\\\ d\n$$\n");
        assert!(out.contains(r"a \\ b"), "{out}");
        assert!(out.contains(r"c \\ d"), "{out}");
    }

    #[test]
    fn emoji_shortcodes() {
        assert!(html(":rocket:\n").contains('🚀'));
    }

    #[test]
    fn raw_html_is_kept() {
        let out = html("<details><summary>More</summary>\n\nHidden\n\n</details>\n");
        assert!(out.contains("<details><summary>More</summary>"), "{out}");
    }

    #[test]
    fn front_matter_is_not_shown() {
        let out = html("---\ntitle: Hello\n---\n# Body\n");
        assert!(!out.contains("title: Hello"), "{out}");
        assert!(out.contains("Body"), "{out}");
    }

    #[test]
    fn obsidian_syntax_stays_text() {
        let out = html("[[Note]] ==mark== %%hidden%%\n");
        assert!(out.contains("[[Note]]"), "{out}");
        assert!(out.contains("==mark=="), "{out}");
        assert!(out.contains("%%hidden%%"), "{out}");
    }
}
