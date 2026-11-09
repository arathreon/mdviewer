//! Obsidian wiki links: `[[note]]`, `[[folder/note|alias]]`, `[[note#Heading]]`,
//! `[[#Heading]]` and image embeds `![[image.png]]`.
//!
//! Privacy: when a page opens, the viewer does **not** list any folder. It only checks
//! whether a few exact files exist ([`Resolver`]): a relative path, the note folder,
//! the vault root and Obsidian's attachment folder. A link that is not found there
//! stays a wiki link (`data-wikilink`, shown faded). Only when the user clicks such a
//! link does the viewer search the vault ([`search`], [`VaultIndex`]).
//!
//! A resolved link becomes a normal link (or image) to the page URL of the file, so
//! that clicks work like any other link.

use std::fs;
use std::path::{Component, Path, PathBuf};

use comrak::Anchorizer;
use comrak::nodes::{AstNode, NodeLink, NodeValue};

use crate::obsidian_vaults;
use crate::render::RenderEnv;
use crate::site::media_type;

/// The click search stops after this many files, so that a huge folder cannot block
/// the viewer.
pub const MAX_INDEX_FILES: usize = 50_000;

/// Resolves the wiki links of a parsed document with direct checks only.
pub fn resolve<'a>(root: &'a AstNode<'a>, env: &RenderEnv<'_>) {
    let links: Vec<_> = root
        .descendants()
        .filter(|node| matches!(node.data().value, NodeValue::WikiLink(_)))
        .collect();
    if links.is_empty() {
        return;
    }
    let Some(resolver) = env
        .document
        .and_then(|document| Resolver::new(env.vault, document))
    else {
        return;
    };
    for node in links {
        let (url, embed) = match &node.data().value {
            NodeValue::WikiLink(link) => (link.url.clone(), link.embed),
            _ => continue,
        };
        match resolved(&Target::parse(&url), embed, &resolver, env) {
            Some(value) => node.data_mut().value = value,
            None => log::debug!("wiki link not found by direct checks: {url}"),
        }
    }
}

/// The result of a click on an unresolved wiki link.
#[derive(Debug, PartialEq, Eq)]
pub struct Found {
    /// The file that the link names.
    pub file: PathBuf,
    /// The heading id, if the link names a heading.
    pub fragment: Option<String>,
}

/// Searches the vault (or the document folder) for a wiki link target. Only for a
/// click of the user: this lists the files of the vault.
pub fn search(vault: Option<&Path>, document: &Path, target: &str) -> Option<Found> {
    let target = Target::parse(target);
    if target.name.is_empty() {
        return None;
    }
    let resolver = Resolver::new(vault, document)?;
    let file = resolver.find(target.name).or_else(|| {
        let from = document
            .parent()
            .and_then(|dir| dir.strip_prefix(&resolver.vault).ok())
            .map(slash_path);
        VaultIndex::build(&resolver.vault, MAX_INDEX_FILES).find(target.name, from.as_deref())
    })?;
    Some(Found {
        file,
        fragment: target.heading.map(slug),
    })
}

/// The new node for a wiki link, or `None` if the target is not found.
fn resolved(
    target: &Target<'_>,
    embed: bool,
    resolver: &Resolver,
    env: &RenderEnv<'_>,
) -> Option<NodeValue> {
    // [[#Heading]]: a heading in the same note.
    if target.name.is_empty() {
        return target
            .heading
            .map(|heading| link(format!("#{}", slug(heading))));
    }
    let file = resolver.find(target.name)?;
    let url_path = env.site?.url_path_of(&file)?;
    if embed && media_type(&file).is_some() {
        // The children of the wiki link (its text) become the alt text.
        return Some(NodeValue::Image(Box::new(NodeLink {
            url: url_path,
            title: String::new(),
        })));
    }
    let url = match target.heading {
        Some(heading) => format!("{url_path}#{}", slug(heading)),
        None => url_path,
    };
    Some(link(url))
}

/// A normal Markdown link node (without a title).
fn link(url: String) -> NodeValue {
    NodeValue::Link(Box::new(NodeLink {
        url,
        title: String::new(),
    }))
}

/// The heading id that comrak gives a heading (GitHub rules), without a number suffix.
fn slug(heading: &str) -> String {
    Anchorizer::new().anchorize(heading)
}

/// The parts of a wiki link target: `name#heading`.
#[derive(Debug, PartialEq, Eq)]
struct Target<'a> {
    /// The note or file name before `#`, trimmed. Empty for `[[#Heading]]`.
    name: &'a str,
    /// The last heading of `#h1#h2`. `None` for block references (`#^id`), which are
    /// not supported: the note opens at the top.
    heading: Option<&'a str>,
}

impl<'a> Target<'a> {
    /// Splits a wiki link target (the part before `|`) into name and heading.
    fn parse(url: &'a str) -> Target<'a> {
        let (name, heading) = match url.split_once('#') {
            Some((name, rest)) => (name, rest.rsplit('#').next()),
            None => (url, None),
        };
        Target {
            name: name.trim(),
            heading: heading
                .map(str::trim)
                .filter(|heading| !heading.is_empty() && !heading.starts_with('^')),
        }
    }
}

/// Finds wiki link targets with direct checks only: no folder is listed.
#[derive(Debug)]
pub struct Resolver {
    /// The vault root, or the document folder outside a vault.
    vault: PathBuf,
    /// The folder of the current note.
    note_dir: PathBuf,
    /// Obsidian's attachment folder setting (`attachmentFolderPath`), if any.
    attachments: Option<String>,
}

impl Resolver {
    /// A resolver for links in `document`. Outside a vault, the document folder is the
    /// root. Reads only Obsidian's `app.json` (for the attachment folder), no folder list.
    pub fn new(vault: Option<&Path>, document: &Path) -> Option<Resolver> {
        let note_dir = document.parent()?.to_path_buf();
        let vault = vault.map_or_else(|| note_dir.clone(), Path::to_path_buf);
        let attachments = fs::read_to_string(vault.join(".obsidian").join("app.json"))
            .ok()
            .and_then(|json| {
                obsidian_vaults::string_values(&json, "attachmentFolderPath")
                    .into_iter()
                    .next()
            });
        Some(Resolver {
            vault,
            note_dir,
            attachments,
        })
    }

    /// Checks the places in order and gives the first existing file.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        self.candidates(name)
            .into_iter()
            .find(|candidate| candidate.is_file())
    }

    /// The exact paths that are checked for a link name (all inside the vault).
    fn candidates(&self, name: &str) -> Vec<PathBuf> {
        let file = wanted_file(name);
        // A drive letter or an alternate data stream must never be checked.
        if file.contains(':') {
            return Vec::new();
        }
        let relative = file.starts_with("./") || file.starts_with("../");
        let file = file.trim_start_matches('/');
        let mut bases = Vec::new();
        if relative {
            bases.push(self.note_dir.clone());
        } else {
            bases.push(self.note_dir.clone());
            bases.push(self.vault.clone());
            if let Some(folder) = self.attachment_dir() {
                bases.push(folder);
            }
        }
        bases
            .iter()
            .filter_map(|base| inside(&self.vault, base, file))
            .collect()
    }

    /// The attachment folder: `/` is the vault root, `./` and `./sub` are below the note.
    fn attachment_dir(&self) -> Option<PathBuf> {
        let setting = self.attachments.as_deref()?.replace('\\', "/");
        let folder = match setting.strip_prefix("./") {
            Some(below_note) => inside(&self.vault, &self.note_dir, below_note),
            None if setting == "." => Some(self.note_dir.clone()),
            None => inside(&self.vault, &self.vault, setting.trim_start_matches('/')),
        };
        folder.filter(|folder| folder != &self.vault && folder != &self.note_dir)
    }
}

/// `base` joined with the relative `/`-path `relative`, if the result stays inside
/// `vault`. Handles `.` and `..` by text, without the disk.
fn inside(vault: &Path, base: &Path, relative: &str) -> Option<PathBuf> {
    let mut path = base.to_path_buf();
    for part in Path::new(relative).components() {
        match part {
            Component::Normal(name) => path.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                path.pop();
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    path.starts_with(vault).then_some(path)
}

/// The files below a vault (or folder), as relative paths with `/`. Only for the
/// click search.
#[derive(Debug)]
pub struct VaultIndex {
    /// The folder that was searched.
    root: PathBuf,
    /// The files below `root`, relative, with `/`, sorted.
    files: Vec<String>,
}

impl VaultIndex {
    /// Lists the files below `root`, at most `limit` of them. Hidden folders (`.obsidian`,
    /// `.trash`, `.git`), `node_modules`, and links and junctions are skipped; skipping
    /// links also protects against folder loops.
    pub fn build(root: &Path, limit: usize) -> VaultIndex {
        let mut files = Vec::new();
        let mut folders = vec![PathBuf::new()];
        'walk: while let Some(relative) = folders.pop() {
            let Ok(entries) = fs::read_dir(root.join(&relative)) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if kind.is_symlink() || name.starts_with('.') {
                    continue;
                }
                let path = relative.join(name);
                if kind.is_dir() {
                    if name != "node_modules" {
                        folders.push(path);
                    }
                } else {
                    if files.len() == limit {
                        log::warn!(
                            "wiki link search stopped after {limit} files in {}",
                            root.display()
                        );
                        break 'walk;
                    }
                    files.push(slash_path(&path));
                }
            }
        }
        files.sort();
        VaultIndex {
            root: root.to_path_buf(),
            files,
        }
    }

    /// Finds the file for a wiki link name. `from` is the folder of the current note,
    /// relative to the vault, with `/`.
    ///
    /// Order (as in Obsidian): a file in the same folder, then the shortest path, then
    /// alphabetical order. Letter case is ignored.
    pub fn find(&self, name: &str, from: Option<&str>) -> Option<PathBuf> {
        let wanted = wanted_file(name);
        let wanted = wanted
            .trim_start_matches("./")
            .trim_start_matches('/')
            .to_lowercase();
        let suffix = format!("/{wanted}");
        let from = from.map(str::to_lowercase);
        let candidates: Vec<&String> = self
            .files
            .iter()
            .filter(|file| {
                let file = file.to_lowercase();
                file == wanted || file.ends_with(&suffix)
            })
            .collect();
        let in_same_folder = candidates.iter().find(|file| {
            let folder = file.rsplit_once('/').map_or("", |(folder, _)| folder);
            Some(folder.to_lowercase()) == from
        });
        let best = in_same_folder.or_else(|| {
            candidates
                .iter()
                .min_by_key(|file| file.matches('/').count())
        })?;
        // One part at a time, so that the path has only Windows separators.
        Some(
            best.split('/')
                .fold(self.root.clone(), |path, part| path.join(part)),
        )
    }
}

/// The file name for a link name, with `/`: `note` → `note.md`, `image.png` stays.
fn wanted_file(name: &str) -> String {
    let name = name.trim().replace('\\', "/");
    let extension = Path::new(&name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let has_file_type = match extension.as_deref() {
        Some("md" | "pdf" | "canvas") => true,
        Some(_) => media_type(Path::new(&name)).is_some(),
        None => false,
    };
    if has_file_type {
        name
    } else {
        // "v1.2 notes" is a note, not a file of type "2 notes".
        format!("{name}.md")
    }
}

/// A relative path as text with `/` between the parts, as wiki links write paths.
fn slash_path(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{Feature, RawHtml, WikiLinks};
    use crate::render::render_in;
    use crate::site::Site;
    use crate::test_util::TempDir;

    fn write(root: &Path, files: &[&str]) {
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
    }

    /// A vault with `files`, an optional `app.json`, and the note `document`.
    fn vault(
        name: &str,
        files: &[&str],
        app_json: Option<&str>,
        document: &str,
    ) -> (TempDir, PathBuf, PathBuf) {
        let dir = TempDir::new(name);
        let vault = dir.path().join("Vault");
        write(&vault, files);
        write(&vault, &[document]);
        if let Some(json) = app_json {
            fs::create_dir_all(vault.join(".obsidian")).unwrap();
            fs::write(vault.join(".obsidian/app.json"), json).unwrap();
        }
        let document = vault.join(document);
        (dir, vault, document)
    }

    fn render_note(vault: &Path, document: &Path, markdown: &str) -> String {
        let site = Site::for_document(vault, document);
        let env = RenderEnv {
            document: Some(document),
            vault: Some(vault),
            site: Some(&site),
        };
        let features: [&dyn Feature; 2] = [&RawHtml, &WikiLinks];
        render_in(markdown, &features, &env)
    }

    #[test]
    fn target_parts() {
        assert_eq!(
            Target::parse("Note"),
            Target {
                name: "Note",
                heading: None
            }
        );
        assert_eq!(
            Target::parse("Note#My Heading"),
            Target {
                name: "Note",
                heading: Some("My Heading")
            }
        );
        assert_eq!(Target::parse("Note#H1#H2").heading, Some("H2"));
        assert_eq!(Target::parse("Note#^block-id").heading, None);
        assert_eq!(Target::parse("#Local").name, "");
        assert_eq!(Target::parse(" Note # ").heading, None);
    }

    #[test]
    fn wanted_file_adds_md_only_for_notes() {
        assert_eq!(wanted_file("Note"), "Note.md");
        assert_eq!(wanted_file("Note.md"), "Note.md");
        assert_eq!(wanted_file("v1.2 notes"), "v1.2 notes.md");
        assert_eq!(wanted_file("image.PNG"), "image.PNG");
        assert_eq!(wanted_file("doc.pdf"), "doc.pdf");
        assert_eq!(wanted_file(r"folder\Note"), "folder/Note.md");
    }

    #[test]
    fn same_folder_vault_root_and_link_path_are_checked_directly() {
        let files = ["Daily/Same.md", "Root.md", "Projects/Plan.md"];
        let (_dir, vault, document) = vault("wiki-direct", &files, None, "Daily/today.md");
        let html = render_note(
            &vault,
            &document,
            "[[Same]] [[root|alias]] [[Projects/Plan#Next Step]]\n",
        );
        assert!(
            html.contains(r#"<a href="/Daily/Same.md">Same</a>"#),
            "{html}"
        );
        assert!(html.contains(r#"<a href="/Root.md">alias</a>"#), "{html}");
        assert!(
            html.contains(r#"href="/Projects/Plan.md#next-step""#),
            "{html}"
        );
        assert!(!html.contains("data-wikilink"), "{html}");
    }

    #[test]
    fn a_bare_name_in_another_folder_is_not_searched_on_open() {
        // Obsidian's "shortest path" links: only the click search finds these.
        let (_dir, vault, document) = vault(
            "wiki-no-listing",
            &["Projects/Plan.md"],
            None,
            "Daily/today.md",
        );
        let html = render_note(&vault, &document, "[[Plan]]\n");
        assert!(html.contains("data-wikilink"), "{html}");
    }

    #[test]
    fn relative_links_start_in_the_note_folder() {
        let files = ["Daily/sub/Near.md", "Sibling.md"];
        let (_dir, vault, document) = vault("wiki-relative", &files, None, "Daily/today.md");
        let html = render_note(&vault, &document, "[[./sub/Near]] [[../Sibling]]\n");
        assert!(html.contains(r#"href="/Daily/sub/Near.md""#), "{html}");
        assert!(html.contains(r#"href="/Sibling.md""#), "{html}");
    }

    #[test]
    fn image_embeds_use_the_attachment_folder_setting() {
        let files = ["Attachments/My image.png", "Other.md"];
        let json = r#"{"attachmentFolderPath":"Attachments"}"#;
        let (_dir, vault, document) =
            vault("wiki-attachments", &files, Some(json), "notes/today.md");
        let html = render_note(&vault, &document, "![[My image.png]] ![[Other]]\n");
        assert!(
            html.contains(r#"<img src="/Attachments/My%20image.png" alt="My image.png" />"#),
            "{html}"
        );
        assert!(html.contains(r#"<a href="/Other.md">Other</a>"#), "{html}");
    }

    #[test]
    fn attachment_folder_below_the_note() {
        let files = ["notes/assets/pic.png"];
        let json = r#"{"attachmentFolderPath":"./assets"}"#;
        let (_dir, vault, document) = vault(
            "wiki-attachments-below",
            &files,
            Some(json),
            "notes/today.md",
        );
        let html = render_note(&vault, &document, "![[pic.png]]\n");
        assert!(
            html.contains(r#"<img src="/notes/assets/pic.png""#),
            "{html}"
        );
    }

    #[test]
    fn heading_links_in_the_same_note() {
        let (_dir, vault, document) = vault("wiki-local-heading", &[], None, "today.md");
        let html = render_note(&vault, &document, "[[#Local Part]]\n");
        assert!(
            html.contains(r##"<a href="#local-part">#Local Part</a>"##),
            "{html}"
        );
    }

    #[test]
    fn checks_never_leave_the_vault() {
        let dir = TempDir::new("wiki-escape");
        write(dir.path(), &["secret.md"]);
        let vault = dir.path().join("Vault");
        write(&vault, &["today.md"]);
        let resolver = Resolver::new(Some(&vault), &vault.join("today.md")).unwrap();
        for name in [
            "../secret",
            "./../secret",
            "C:/Windows/win",
            r"\\server\share\x",
            "a:b",
        ] {
            assert_eq!(resolver.find(name), None, "{name}");
            for candidate in resolver.candidates(name) {
                assert!(
                    candidate.starts_with(&vault),
                    "{name}: {}",
                    candidate.display()
                );
            }
        }
    }

    #[test]
    fn click_search_finds_bare_names_and_headings() {
        let files = ["Projects/Plan.md", "Archive/deep/Plan.md"];
        let (_dir, vault, document) = vault("wiki-search", &files, None, "Daily/today.md");
        assert_eq!(
            search(Some(&vault), &document, "Plan#Next Step"),
            Some(Found {
                file: vault.join("Projects/Plan.md"),
                fragment: Some("next-step".to_owned()),
            })
        );
        let found = search(Some(&vault), &document, "Plan").unwrap();
        assert!(
            !found.file.to_str().unwrap().contains('/'),
            "{}",
            found.file.display()
        );
        assert_eq!(search(Some(&vault), &document, "Missing"), None);
        assert_eq!(search(Some(&vault), &document, "#Only heading"), None);
    }

    #[test]
    fn click_search_prefers_same_folder_then_shortest_path() {
        let files = ["a.md", "Daily/a.md", "x/y/b.md", "x/b.md"];
        let (_dir, vault, document) = vault("wiki-order", &files, None, "Daily/today.md");
        let index = VaultIndex::build(&vault, 100);
        assert_eq!(
            index.find("a", Some("Daily")),
            Some(vault.join("Daily/a.md"))
        );
        assert_eq!(index.find("b", Some("Daily")), Some(vault.join("x/b.md")));
        assert_eq!(index.find("B", None), Some(vault.join("x/b.md")));
        let _ = document;
    }

    #[test]
    fn click_search_skips_hidden_folders_and_node_modules() {
        let files = [".obsidian/x.md", ".trash/y.md", "node_modules/z.md"];
        let (_dir, vault, document) = vault("wiki-hidden", &files, None, "today.md");
        for name in ["x", "y", "z"] {
            assert_eq!(search(Some(&vault), &document, name), None, "{name}");
        }
    }

    #[test]
    fn index_stops_at_the_limit() {
        let dir = TempDir::new("wiki-limit");
        write(dir.path(), &["a.md", "b.md", "c.md"]);
        assert_eq!(VaultIndex::build(dir.path(), 2).files.len(), 2);
        assert_eq!(VaultIndex::build(dir.path(), 10).files.len(), 3);
    }

    #[test]
    fn junctions_are_not_followed() {
        let dir = TempDir::new("wiki-junction");
        let vault = dir.path().join("Vault");
        write(&vault, &["a.md"]);
        // A junction from the vault to itself would make an endless loop.
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(vault.join("loop"))
            .arg(&vault)
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
        assert_eq!(VaultIndex::build(&vault, 1000).files, ["a.md"]);
    }

    #[test]
    fn without_a_document_links_stay_unresolved() {
        let features: [&dyn Feature; 1] = [&WikiLinks];
        let html = render_in("[[Note]]\n", &features, &RenderEnv::default());
        assert!(html.contains("data-wikilink"), "{html}");
    }
}
