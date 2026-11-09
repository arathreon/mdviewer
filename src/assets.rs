//! Files that the viewer itself serves at `https://mdviewer.localhost/_mdviewer/<path>`:
//! the content script (`render.mjs`) and the third-party libraries (`assets/vendor`).
//!
//! The CSP allows scripts from this path only. Thus, the path must never serve files
//! that come from a document folder.

include!(concat!(env!("OUT_DIR"), "/vendor_files.rs"));

/// The URL path below which the viewer files are served.
pub const PREFIX: &str = "/_mdviewer/";

/// The content script, which loads the libraries only when the page needs them.
pub const RENDER_SCRIPT: &str = "render.mjs";

/// Our own files (not third-party).
static OWN_FILES: &[(&str, &[u8])] = &[(RENDER_SCRIPT, include_bytes!("../assets/render.mjs"))];

/// An embedded file.
#[derive(Debug, PartialEq, Eq)]
pub struct Asset {
    /// The `Content-Type` header value, for example `text/javascript; charset=utf-8`.
    pub content_type: &'static str,
    /// The file content, embedded in the program.
    pub content: &'static [u8],
}

/// Finds the file for a URL path, for example `/_mdviewer/katex/katex.min.js`.
pub fn find(url_path: &str) -> Option<Asset> {
    let name = url_path.strip_prefix(PREFIX)?;
    let content = OWN_FILES
        .iter()
        .chain(VENDOR_FILES)
        .find(|(file, _)| *file == name)
        .map(|(_, content)| *content)?;
    Some(Asset {
        content_type: content_type(name)?,
        content,
    })
}

/// The content type for a file name. Scripts must have a JavaScript type: the page
/// sends `X-Content-Type-Options: nosniff`, so the browser does not guess.
pub fn content_type(name: &str) -> Option<&'static str> {
    let extension = name.rsplit_once('.')?.1;
    match extension {
        "js" | "mjs" => Some("text/javascript; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "woff2" => Some("font/woff2"),
        // Icons of alerts and callouts (CSS masks).
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

/// All served file names, for tests and diagnostics.
pub fn names() -> impl Iterator<Item = &'static str> {
    OWN_FILES.iter().chain(VENDOR_FILES).map(|(name, _)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// All files below `dir` with a served extension, as paths with `/`.
    fn files_on_disk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = format!("{prefix}{}", entry.file_name().to_str().unwrap());
            if entry.path().is_dir() {
                files_on_disk(&entry.path(), &format!("{name}/"), out);
            } else if content_type(&name).is_some() {
                out.push(name);
            }
        }
    }

    #[test]
    fn every_vendor_file_is_embedded() {
        let mut on_disk = Vec::new();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/vendor");
        files_on_disk(&dir, "", &mut on_disk);
        on_disk.sort();
        let mut embedded: Vec<String> = VENDOR_FILES
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        embedded.sort();
        assert_eq!(embedded, on_disk);
        assert!(embedded.len() > 100, "{}", embedded.len());
    }

    #[test]
    fn the_libraries_and_the_render_script_are_found() {
        for path in [
            "/_mdviewer/render.mjs",
            "/_mdviewer/katex/katex.min.js",
            "/_mdviewer/katex/katex.min.css",
            "/_mdviewer/katex/fonts/KaTeX_Main-Regular.woff2",
            "/_mdviewer/mermaid/mermaid.esm.min.mjs",
            "/_mdviewer/highlight/highlight.min.js",
        ] {
            let asset = find(path).unwrap_or_else(|| panic!("{path} missing"));
            assert!(!asset.content.is_empty(), "{path}");
        }
    }

    #[test]
    fn every_embedded_file_has_a_content_type() {
        for name in names() {
            assert!(content_type(name).is_some(), "{name}");
        }
    }

    #[test]
    fn content_types_are_exact() {
        assert_eq!(
            content_type("a.mjs"),
            Some("text/javascript; charset=utf-8")
        );
        assert_eq!(content_type("a.js"), Some("text/javascript; charset=utf-8"));
        assert_eq!(content_type("a.css"), Some("text/css; charset=utf-8"));
        assert_eq!(content_type("a.woff2"), Some("font/woff2"));
        assert_eq!(content_type("a.svg"), Some("image/svg+xml"));
        assert_eq!(content_type("LICENSE"), None);
        assert_eq!(content_type("a.map"), None);
    }

    #[test]
    fn other_paths_are_not_found() {
        for path in [
            "/_mdviewer/",
            "/_mdviewer/LICENSE",
            "/_mdviewer/katex/LICENSE",
            "/_mdviewer/../Cargo.toml",
            "/_mdviewer//katex/katex.min.js",
            "/katex/katex.min.js",
            "/_MDVIEWER/render.mjs",
            "/",
        ] {
            assert_eq!(find(path), None, "{path}");
        }
    }

    #[test]
    fn every_file_that_the_style_sheet_uses_is_embedded() {
        // For example: mask-image: url("/_mdviewer/octicons/info-16.svg").
        let css = include_str!("../assets/style.css");
        let urls: Vec<&str> = css
            .split("url(\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect();
        assert!(urls.len() >= 18, "{urls:?}");
        for url in urls {
            assert!(url.starts_with(PREFIX), "{url}");
            assert!(find(url).is_some(), "{url} is not embedded");
        }
    }

    #[test]
    fn mermaid_chunks_are_reachable_with_relative_imports() {
        // The entry file imports "./chunks/mermaid.esm.min/chunk-*.mjs".
        let entry = find("/_mdviewer/mermaid/mermaid.esm.min.mjs").unwrap();
        let text = std::str::from_utf8(entry.content).unwrap();
        let import = text
            .split("from\"./")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        assert!(
            find(&format!("/_mdviewer/mermaid/{import}")).is_some(),
            "{import}"
        );
    }
}
