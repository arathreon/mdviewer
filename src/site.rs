//! The address space of a page: its URL, and the local files that it can load.
//!
//! The page URL follows the position of the document below its root folder (the vault,
//! the Git repository or the document folder). For `D:\Notes\Daily\today.md` with the
//! root `D:\Notes`, the page is `https://mdviewer.localhost/Daily/today.md`. Thus, the
//! browser resolves relative paths (`../img/a.png`) exactly as the file system does.
//!
//! Security: the page can load only image and media files, and only from inside the
//! root. The check uses the resolved path on disk ([`Site::servable_file`]), so that
//! `..`, encoded separators, links, junctions and device names cannot get out.

use std::fs;
use std::path::{Component, Path, PathBuf};

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

/// The origin of all pages. It depends on the web view of the system (see `platform`).
pub const ORIGIN: &str = crate::platform::ORIGIN;

/// The largest local file that a page can load.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Characters that are encoded in one path segment of a page URL.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'\\')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'|')
    .add(b'^')
    .add(b'[')
    .add(b']')
    .add(b'\'');

/// The address space of one page: its URL path and the root folder of its local files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The root folder, resolved (`fs::canonicalize`, normal form). `None` for message pages.
    root: Option<PathBuf>,
    /// The URL path of the page, for example `/Daily/today.md`.
    page_path: String,
}

impl Site {
    /// The site of a message page (no file): `https://mdviewer.localhost/`, no local files.
    pub fn none() -> Site {
        Site {
            root: None,
            page_path: "/".to_owned(),
        }
    }

    /// The site of a document below `root`. If the folders cannot be resolved, the
    /// page has no local files (fail closed).
    pub fn for_document(root: &Path, document: &Path) -> Site {
        let resolved = resolve(root).and_then(|root| Ok((root, resolve(document)?)));
        match resolved {
            Ok((root, document)) => match url_path(&root, &document) {
                Some(page_path) => Site {
                    root: Some(root),
                    page_path,
                },
                None => {
                    log::warn!("{} is not below {}", document.display(), root.display());
                    Site::none()
                }
            },
            Err(error) => {
                log::warn!("cannot resolve {}: {error}", document.display());
                Site::none()
            }
        }
    }

    /// The root folder (resolved). The page can load files only from below it.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The URL path of the page, for example `/Daily/today.md`.
    pub fn page_path(&self) -> &str {
        &self.page_path
    }

    /// The full URL of the page.
    pub fn page_url(&self) -> String {
        format!("{ORIGIN}{}", self.page_path)
    }

    /// Gives `true` only for the page itself, with or without a `#fragment`.
    pub fn is_page_url(&self, url: &str) -> bool {
        let without_fragment = url.split_once('#').map_or(url, |(before, _)| before);
        without_fragment
            .strip_prefix(ORIGIN)
            .is_some_and(|path| self.is_page_path(path))
    }

    /// Gives `true` if a request path is the page. The paths are compared decoded,
    /// because the browser can encode the same name differently (`%c3%a9`, `%C3%A9`).
    pub fn is_page_path(&self, path: &str) -> bool {
        let decoded = |path| percent_decode_str(path).decode_utf8().ok();
        path.starts_with('/')
            && decoded(path).is_some()
            && decoded(path) == decoded(&self.page_path)
    }

    /// The URL path of a file below the root, for example `/img/a%20b.png`.
    pub fn url_path_of(&self, file: &Path) -> Option<String> {
        let root = self.root.as_deref()?;
        url_path(root, &resolve(file).ok()?)
    }

    /// The local path for a URL path, without any check on disk. `None` if the URL
    /// path has a segment that is not a plain file or folder name.
    pub fn local_path(&self, url_path: &str) -> Option<PathBuf> {
        let mut path = self.root.clone()?;
        for segment in url_path.strip_prefix('/')?.split('/') {
            path.push(decode_segment(segment)?);
        }
        Some(path)
    }

    /// The file for a request of the page, after all security checks.
    pub fn servable_file(&self, url_path: &str) -> Result<PathBuf, &'static str> {
        let root = self.root.as_deref().ok_or("page has no folder")?;
        let path = self
            .local_path(url_path)
            .ok_or("address is not a plain path")?;
        media_type(&path).ok_or("file type is not allowed")?;
        // Resolves `..`, links, junctions and short names; fails for missing files.
        let resolved = resolve(&path).map_err(|_| "file not found")?;
        if !resolved.starts_with(root) {
            return Err("file is outside the root folder");
        }
        let metadata = fs::metadata(&resolved).map_err(|_| "file not found")?;
        if !metadata.is_file() {
            return Err("not a file");
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err("file is too large");
        }
        // The resolved name can differ (for example a link to an .exe): check again.
        media_type(&resolved).ok_or("file type is not allowed")?;
        Ok(resolved)
    }
}

/// `fs::canonicalize` in the normal path form. All checks in this file use this form.
fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path).map(crate::platform::normal_form)
}

/// The content type of a local file that a page can load (images and media only).
pub fn media_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" | "apng" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        _ => return None,
    })
}

/// The URL path of `file` below `root` (both resolved), with each name encoded.
fn url_path(root: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(root).ok()?;
    let mut out = String::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        out.push('/');
        out.extend(utf8_percent_encode(name.to_str()?, SEGMENT));
    }
    if out.is_empty() {
        out.push('/');
    }
    Some(out)
}

/// Decodes one URL path segment into a plain file or folder name.
///
/// Rejects empty names, `.` and `..`, and names with characters that have a meaning for
/// Windows paths: separators (also encoded as `%2F` or `%5C`), `:` (drive letters and
/// alternate data streams) and control characters.
fn decode_segment(segment: &str) -> Option<String> {
    let name = percent_decode_str(segment).decode_utf8().ok()?;
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':') || c.is_control());
    (!bad).then(|| name.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// A vault with notes, images and things that must not be served.
    fn vault() -> (TempDir, Site) {
        let dir = TempDir::new("site");
        let root = dir.path().join("Vault");
        for folder in ["Daily", "img", "Folder With Space", "deep/er"] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        fs::write(root.join("Daily/today.md"), "# Today").unwrap();
        fs::write(root.join("img/a.png"), b"png").unwrap();
        fs::write(root.join("Folder With Space/b c#d.jpg"), b"jpg").unwrap();
        fs::write(root.join("img/script.js"), b"alert(1)").unwrap();
        fs::write(root.join("img/page.html"), b"<script>").unwrap();
        fs::write(dir.path().join("outside.png"), b"png").unwrap();
        let site = Site::for_document(&root, &root.join("Daily/today.md"));
        (dir, site)
    }

    #[test]
    fn page_url_follows_the_position_below_the_root() {
        let (_dir, site) = vault();
        assert_eq!(site.page_path(), "/Daily/today.md");
        assert_eq!(site.page_url(), "https://mdviewer.localhost/Daily/today.md");
        assert!(site.is_page_url("https://mdviewer.localhost/Daily/today.md#heading"));
        assert!(!site.is_page_url("https://mdviewer.localhost/Daily/other.md"));
        assert!(!site.is_page_url("https://mdviewer.localhost/"));
        assert!(!site.is_page_url("https://mdviewer.localhost.example.com/Daily/today.md"));
        assert!(!site.is_page_url("http://mdviewer.localhost/Daily/today.md"));
        assert!(
            !site.is_page_url("https://example.com/#https://mdviewer.localhost/Daily/today.md")
        );
        assert!(!site.is_page_url(""));
    }

    #[test]
    fn page_paths_are_compared_decoded() {
        let dir = TempDir::new("site-encoded");
        fs::create_dir_all(dir.path().join("Été")).unwrap();
        let document = dir.path().join("Été/a b.md");
        fs::write(&document, "x").unwrap();
        let site = Site::for_document(dir.path(), &document);
        assert_eq!(site.page_path(), "/%C3%89t%C3%A9/a%20b.md");
        assert!(site.is_page_path("/%c3%89t%c3%a9/a%20b.md"));
        assert!(site.is_page_path("/%C3%89t%C3%A9/a b.md"));
        assert!(!site.is_page_path("/%C3%89t%C3%A9/a%20c.md"));
        assert!(!site.is_page_path("/%FF/a.md"));
        assert!(Site::none().is_page_url("https://mdviewer.localhost/"));
    }

    #[test]
    fn names_are_encoded_and_decoded_both_ways() {
        let (_dir, site) = vault();
        let root = site.root().unwrap().to_path_buf();
        let file = root.join("Folder With Space/b c#d.jpg");
        let path = site.url_path_of(&file).unwrap();
        assert_eq!(path, "/Folder%20With%20Space/b%20c%23d.jpg");
        assert_eq!(site.servable_file(&path).unwrap(), resolve(&file).unwrap());
        let unicode = decode_segment("%C3%A9t%C3%A9").unwrap();
        assert_eq!(unicode, "été");
    }

    #[test]
    fn images_inside_the_root_are_served() {
        let (_dir, site) = vault();
        let file = site.servable_file("/img/a.png").unwrap();
        assert!(file.ends_with("img/a.png") || file.ends_with(r"img\a.png"));
        assert_eq!(media_type(&file), Some("image/png"));
    }

    #[test]
    fn only_media_types_are_served() {
        let (_dir, site) = vault();
        assert_eq!(
            site.servable_file("/img/script.js"),
            Err("file type is not allowed")
        );
        assert_eq!(
            site.servable_file("/img/page.html"),
            Err("file type is not allowed")
        );
        assert_eq!(
            site.servable_file("/Daily/today.md"),
            Err("file type is not allowed")
        );
        assert_eq!(
            site.servable_file("/img/missing.png"),
            Err("file not found")
        );
    }

    #[test]
    fn paths_cannot_leave_the_root() {
        let (_dir, site) = vault();
        for attack in [
            "/../outside.png",
            "/img/../../outside.png",
            "/%2E%2E/outside.png",
            "/img/..%2F..%2Foutside.png",
            "/img/..%5C..%5Coutside.png",
            "/C:%5CWindows%5Cwin.ini.png",
            "/img/a.png:stream.png",
            "/img//a.png",
            "/img/./a.png",
            "/img/a%00.png",
            "img/a.png",
        ] {
            assert!(site.servable_file(attack).is_err(), "{attack}");
        }
    }

    #[test]
    fn device_names_are_not_served() {
        let (_dir, site) = vault();
        for device in ["/CON.png", "/NUL.png", "/img/COM1.png", "/img/aux.jpg"] {
            assert!(site.servable_file(device).is_err(), "{device}");
        }
    }

    #[test]
    #[cfg(windows)]
    fn links_out_of_the_root_are_not_served() {
        let (dir, site) = vault();
        let link = site.root().unwrap().join("img/link.png");
        // Symbolic links need a Windows setting (Developer Mode); skip if not allowed.
        if std::os::windows::fs::symlink_file(dir.path().join("outside.png"), &link).is_ok() {
            assert_eq!(
                site.servable_file("/img/link.png"),
                Err("file is outside the root folder")
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn junctions_out_of_the_root_are_not_served() {
        let (dir, site) = vault();
        let outside = dir.path().join("outside-folder");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.png"), b"png").unwrap();
        let junction = site.root().unwrap().join("img").join("junction");
        // A directory junction needs no admin rights (unlike a symbolic link).
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
        assert_eq!(
            site.servable_file("/img/junction/secret.png"),
            Err("file is outside the root folder")
        );
    }

    #[test]
    fn roots_and_local_paths_are_in_normal_form() {
        let (_dir, site) = vault();
        let root = site.root().unwrap().to_str().unwrap().to_owned();
        assert!(!root.starts_with(r"\\?\"), "{root}");
        let local = site.local_path("/img/a.png").unwrap();
        assert!(
            !local.to_str().unwrap().starts_with(r"\\?\"),
            "{}",
            local.display()
        );
    }

    #[test]
    fn message_pages_have_no_files() {
        let site = Site::none();
        assert_eq!(site.page_url(), "https://mdviewer.localhost/");
        assert_eq!(site.servable_file("/img/a.png"), Err("page has no folder"));
        assert_eq!(site.local_path("/a.md"), None);
    }

    #[test]
    fn a_document_outside_its_root_gets_no_files() {
        let (dir, _site) = vault();
        let site = Site::for_document(
            &dir.path().join("Vault/img"),
            &dir.path().join("Vault/Daily/today.md"),
        );
        assert_eq!(site, Site::none());
    }

    #[test]
    fn media_types_ignore_case_and_reject_others() {
        assert_eq!(media_type(Path::new("A.PNG")), Some("image/png"));
        assert_eq!(media_type(Path::new("x.svg")), Some("image/svg+xml"));
        assert_eq!(media_type(Path::new("x.mp4")), Some("video/mp4"));
        for name in ["x.exe", "x.js", "x.html", "x.md", "x", "x.png.lnk"] {
            assert_eq!(media_type(Path::new(name)), None, "{name}");
        }
    }
}
