//! Markdown to HTML: parse, let the features change the document, write HTML.

use std::path::Path;

use comrak::{Arena, Options, format_html, parse_document};

use crate::feature::Feature;
use crate::site::Site;

/// What the features may know about the document while it is rendered.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderEnv<'a> {
    /// The Markdown file (absolute), if the text comes from a file.
    pub document: Option<&'a Path>,
    /// The Obsidian vault of the document, for wiki links.
    pub vault: Option<&'a Path>,
    /// The address space of the page, for the URLs of local files.
    pub site: Option<&'a Site>,
}

/// Makes the `comrak` options for a list of features.
pub fn options(features: &[&dyn Feature]) -> Options<'static> {
    let mut options = Options::default();
    for feature in features {
        feature.configure(&mut options);
    }
    options
}

/// Converts Markdown to an HTML fragment (without `<html>` and `<body>`), for text
/// that is not in a file.
pub fn render(markdown: &str, features: &[&dyn Feature]) -> String {
    render_in(markdown, features, &RenderEnv::default())
}

/// Converts Markdown to an HTML fragment, with knowledge about the document.
pub fn render_in(markdown: &str, features: &[&dyn Feature], env: &RenderEnv<'_>) -> String {
    let options = options(features);
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options);
    for feature in features {
        feature.transform(root, env);
    }
    let mut html = String::with_capacity(markdown.len() * 2);
    format_html(root, &options, &mut html).expect("writing to a String does not fail");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{RawHtml, Tables};

    #[test]
    fn without_features_raw_html_is_removed_and_tables_are_text() {
        let html = render("<b>x</b>\n\n| a |\n|---|\n| 1 |\n", &[]);
        assert!(html.contains("raw HTML omitted"), "{html}");
        assert!(!html.contains("<table>"), "{html}");
    }

    #[test]
    fn features_are_applied() {
        let html = render("<b>x</b>\n\n| a |\n|---|\n| 1 |\n", &[&RawHtml, &Tables]);
        assert!(html.contains("<b>x</b>"), "{html}");
        assert!(html.contains("<table>"), "{html}");
    }

    #[test]
    fn output_is_the_same_as_comrak_markdown_to_html() {
        let markdown = "# Title\n\n- [x] item\n\n| a |\n|---|\n| 1 |\n\n<i>raw</i>\n";
        let features: [&dyn Feature; 2] = [&RawHtml, &Tables];
        assert_eq!(
            render(markdown, &features),
            comrak::markdown_to_html(markdown, &options(&features))
        );
    }
}
