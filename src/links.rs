//! What a click on a link does. The page script sends every click on a link to Rust
//! (not the navigation handler: a `<meta refresh>` would then open a website without
//! a click).

use std::fs;
use std::path::PathBuf;

use percent_encoding::percent_decode_str;
use url::Url;

use crate::network::PAGE_HOST;
use crate::page::is_markdown_file;
use crate::site::Site;

/// The schemes that open in the default program. All others are blocked: many protocol
/// handlers are dangerous (for example `ms-msdt:`, `search-ms:`).
pub const EXTERNAL_SCHEMES: &[&str] = &["http", "https", "mailto"];

#[derive(Debug, PartialEq, Eq)]
/// What the viewer does for a clicked link.
pub enum LinkAction {
    /// Show a Markdown file in the viewer, at a heading if `fragment` is given.
    OpenMarkdown {
        /// The Markdown file.
        file: PathBuf,
        /// The heading id after `#`, decoded.
        fragment: Option<String>,
    },
    /// Show a local file or folder in File Explorer. It is never opened or run: a link
    /// to `setup.exe` must not start a program.
    Reveal(PathBuf),
    /// Open in the default program (browser, mail program).
    External(String),
    /// The local file does not exist.
    Missing(PathBuf),
    /// Not allowed, with the reason.
    Blocked(&'static str),
}

/// Decides what a click on `address` (an absolute URL, resolved by the browser against
/// the page URL) does.
pub fn classify(address: &str, site: &Site) -> LinkAction {
    let Ok(url) = Url::parse(address) else {
        return LinkAction::Blocked("address is not valid");
    };
    let local = match url.scheme() {
        "https" if url.host_str() == Some(PAGE_HOST) => match site.local_path(url.path()) {
            Some(path) => path,
            None => return LinkAction::Blocked("not a local file"),
        },
        "file" => {
            // file://server/share: Windows would connect and send the login (NTLM).
            if url.host_str().is_some_and(|host| !host.is_empty()) {
                return LinkAction::Blocked("network file");
            }
            match url.to_file_path() {
                Ok(path) => path,
                Err(()) => return LinkAction::Blocked("address is not valid"),
            }
        }
        scheme if EXTERNAL_SCHEMES.contains(&scheme) => return LinkAction::External(url.into()),
        _ => return LinkAction::Blocked("protocol is not allowed"),
    };
    let fragment = url
        .fragment()
        .map(|fragment| {
            percent_decode_str(fragment)
                .decode_utf8_lossy()
                .into_owned()
        })
        .filter(|fragment| !fragment.is_empty());
    match fs::metadata(&local) {
        Ok(metadata) if metadata.is_file() && is_markdown_file(&local) => {
            LinkAction::OpenMarkdown {
                file: local,
                fragment,
            }
        }
        Ok(_) => LinkAction::Reveal(local),
        Err(_) => LinkAction::Missing(local),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn site() -> (TempDir, Site) {
        let dir = TempDir::new("links");
        let root = dir.path().join("Root");
        fs::create_dir_all(root.join("docs/sub")).unwrap();
        fs::write(root.join("docs/page.md"), "# Page").unwrap();
        fs::write(root.join("docs/other note.md"), "x").unwrap();
        fs::write(root.join("docs/setup.exe"), "MZ").unwrap();
        let site = Site::for_document(&root, &root.join("docs/page.md"));
        (dir, site)
    }

    fn file(site: &Site, relative: &str) -> PathBuf {
        site.local_path(&format!("/{relative}")).unwrap()
    }

    #[test]
    fn web_and_mail_links_open_externally() {
        let (_dir, site) = site();
        for address in [
            "https://example.com/a?b=c#d",
            "http://example.com/",
            "mailto:me@example.com",
        ] {
            assert!(
                matches!(classify(address, &site), LinkAction::External(_)),
                "{address}"
            );
        }
        // A look-alike host is a normal web address, not a local file.
        assert_eq!(
            classify("https://mdviewer.localhost.example.com/x.md", &site),
            LinkAction::External("https://mdviewer.localhost.example.com/x.md".to_owned())
        );
    }

    #[test]
    fn dangerous_protocols_are_blocked() {
        let (_dir, site) = site();
        for address in [
            "javascript:alert(1)",
            "ms-msdt:/id PCWDiagnostic",
            "search-ms:query=x",
            "vbscript:x",
            "ftp://example.com/x",
            "data:text/html,x",
            "shell:startup",
        ] {
            assert_eq!(
                classify(address, &site),
                LinkAction::Blocked("protocol is not allowed"),
                "{address}"
            );
        }
        assert_eq!(
            classify("not a url", &site),
            LinkAction::Blocked("address is not valid")
        );
    }

    #[test]
    fn local_markdown_links_open_in_the_viewer_with_a_fragment() {
        let (_dir, site) = site();
        assert_eq!(
            classify(
                "https://mdviewer.localhost/docs/other%20note.md#My%20Part",
                &site
            ),
            LinkAction::OpenMarkdown {
                file: file(&site, "docs/other note.md"),
                fragment: Some("My Part".to_owned()),
            }
        );
        assert_eq!(
            classify("https://mdviewer.localhost/docs/page.md#", &site),
            LinkAction::OpenMarkdown {
                file: file(&site, "docs/page.md"),
                fragment: None,
            }
        );
    }

    #[test]
    fn other_local_files_and_folders_are_only_shown_in_explorer() {
        let (_dir, site) = site();
        assert_eq!(
            classify("https://mdviewer.localhost/docs/setup.exe", &site),
            LinkAction::Reveal(file(&site, "docs/setup.exe"))
        );
        assert_eq!(
            classify("https://mdviewer.localhost/docs/sub", &site),
            LinkAction::Reveal(file(&site, "docs/sub"))
        );
    }

    #[test]
    fn missing_local_files_are_reported() {
        let (_dir, site) = site();
        assert_eq!(
            classify("https://mdviewer.localhost/docs/gone.md", &site),
            LinkAction::Missing(file(&site, "docs/gone.md"))
        );
    }

    #[test]
    fn file_links_work_locally_but_not_over_the_network() {
        let (dir, site) = site();
        let page = dir.path().join("Root").join("docs").join("page.md");
        let address = Url::from_file_path(&page).unwrap().to_string();
        assert_eq!(
            classify(&address, &site),
            LinkAction::OpenMarkdown {
                file: page,
                fragment: None,
            }
        );
        for network in ["file://server/share/x.md", "file://192.168.1.1/c$/x.md"] {
            assert_eq!(
                classify(network, &site),
                LinkAction::Blocked("network file"),
                "{network}"
            );
        }
    }

    #[test]
    fn message_pages_have_no_local_links() {
        assert_eq!(
            classify("https://mdviewer.localhost/x.md", &Site::none()),
            LinkAction::Blocked("not a local file")
        );
    }
}
