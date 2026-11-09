//! Facts about one document. Detection rules and features read them.
//!
//! All facts are calculated one time, when the context is made. Rules then only read
//! memory and never examine the disk again.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::obsidian_vaults;

/// Obsidian makes this folder in the root folder of each vault.
const OBSIDIAN_DIR: &str = ".obsidian";
/// A folder in a normal Git repository, or a file in a worktree or submodule.
const GIT_MARKER: &str = ".git";

/// The facts about one Markdown document that detection rules and features need.
///
/// All fields are private and calculated when the context is made, so that a rule
/// cannot change them and never needs the disk.
#[derive(Debug, Clone)]
pub struct Context {
    /// The absolute path of the document.
    path: PathBuf,
    /// The Markdown text (UTF-8, without a byte order mark).
    text: String,
    /// The text without code (see [`Context::prose`]).
    prose: String,
    /// The nearest parent folder with an `.obsidian` folder.
    vault_root: Option<PathBuf>,
    /// The vault from Obsidian's list (`obsidian.json`) that contains the document.
    known_vault: Option<PathBuf>,
    /// The nearest parent folder with `.git`.
    repo_root: Option<PathBuf>,
    /// The Obsidian setting "Strict line breaks" of the vault.
    strict_line_breaks: bool,
}

impl Context {
    /// Reads a Markdown file and examines its folders and Obsidian's list of vaults.
    ///
    /// Text that is not valid UTF-8 is repaired with replacement characters, so that a
    /// damaged file still shows. A byte order mark at the start is removed.
    pub fn load(path: &Path) -> io::Result<Self> {
        Self::load_in(path, None, &obsidian_vaults::load_from_env())
    }

    /// Like [`Context::load`], but the folder search does not go above `ceiling`, and
    /// `known_vaults` replaces Obsidian's list of vaults.
    ///
    /// Tests use it with their temporary folder as the ceiling, so that the folders
    /// above it (for example a `.git` folder above `%TEMP%`) and the vault list of the
    /// computer cannot change the result.
    ///
    /// The file is read before the folders are examined, so a missing file causes no
    /// folder search.
    pub fn load_in(
        path: &Path,
        ceiling: Option<&Path>,
        known_vaults: &[PathBuf],
    ) -> io::Result<Self> {
        let path = std::path::absolute(path)?;
        let text = decode(&fs::read(&path)?);
        Ok(Self::with_ceiling(path, text, ceiling).with_known_vaults(known_vaults))
    }

    /// Adds the vault from Obsidian's list (`obsidian.json`) that contains the document.
    pub fn with_known_vaults(mut self, vaults: &[PathBuf]) -> Self {
        self.known_vault = obsidian_vaults::containing(vaults, &self.path).map(Path::to_path_buf);
        self.strict_line_breaks = self.obsidian_vault().is_some_and(read_strict_line_breaks);
        self
    }

    /// Makes a context for text that is already in memory. The folders of `path` are examined.
    pub fn new(path: PathBuf, text: String) -> Self {
        Self::with_ceiling(path, text, None)
    }

    /// Like [`Context::new`], but the folder search does not go above `ceiling`.
    pub fn with_ceiling(path: PathBuf, text: String, ceiling: Option<&Path>) -> Self {
        let markers = find_markers(&path, ceiling);
        let strict_line_breaks = markers
            .vault_root
            .as_deref()
            .is_some_and(read_strict_line_breaks);
        let prose = prose(&text);
        Self {
            path,
            text,
            prose,
            vault_root: markers.vault_root,
            known_vault: None,
            repo_root: markers.repo_root,
            strict_line_breaks,
        }
    }

    /// The full path of the document.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file name, for example `README.md`.
    pub fn file_name(&self) -> Option<&str> {
        self.path.file_name()?.to_str()
    }

    /// The full Markdown text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text without fenced code blocks and inline code.
    ///
    /// Content rules use it, so that syntax shown as an example inside code does not count.
    pub fn prose(&self) -> &str {
        &self.prose
    }

    /// The nearest parent folder that contains an `.obsidian` folder.
    pub fn vault_root(&self) -> Option<&Path> {
        self.vault_root.as_deref()
    }

    /// The vault from Obsidian's list (`obsidian.json`) that contains the document.
    /// It is found also when the vault's settings folder has another name.
    pub fn known_vault(&self) -> Option<&Path> {
        self.known_vault.as_deref()
    }

    /// The Obsidian vault of the document, from either signal. If both give a folder,
    /// the deeper one wins: it is the nearest vault around the document.
    pub fn obsidian_vault(&self) -> Option<&Path> {
        match (self.vault_root(), self.known_vault()) {
            (Some(a), Some(b)) if b.components().count() > a.components().count() => Some(b),
            (Some(a), _) => Some(a),
            (None, b) => b,
        }
    }

    /// The nearest parent folder that contains `.git`.
    pub fn repo_root(&self) -> Option<&Path> {
        self.repo_root.as_deref()
    }

    /// The Obsidian setting "Strict line breaks" of the vault. `false` outside a vault.
    pub fn strict_line_breaks(&self) -> bool {
        self.strict_line_breaks
    }

    /// A context without a folder search, for tests of rules and features.
    #[cfg(test)]
    pub(crate) fn detached(file_name: &str, text: &str) -> Self {
        Self {
            path: PathBuf::from(file_name),
            text: text.to_owned(),
            prose: prose(text),
            vault_root: None,
            known_vault: None,
            repo_root: None,
            strict_line_breaks: false,
        }
    }
}

/// Makes text from file bytes: invalid UTF-8 becomes replacement characters, and a
/// byte order mark at the start is removed.
fn decode(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    match text.strip_prefix('\u{feff}') {
        Some(without_bom) => without_bom.to_owned(),
        None => text.into_owned(),
    }
}

/// The result of the upward folder search ([`find_markers`]).
struct Markers {
    /// The nearest parent folder with an `.obsidian` folder.
    vault_root: Option<PathBuf>,
    /// The nearest parent folder with `.git`.
    repo_root: Option<PathBuf>,
}

/// Examines the folder of the file and each parent folder, up to `ceiling` if it is given.
fn find_markers(file: &Path, ceiling: Option<&Path>) -> Markers {
    let mut markers = Markers {
        vault_root: None,
        repo_root: None,
    };
    let Some(start) = file.parent() else {
        return markers;
    };
    for dir in start.ancestors() {
        if ceiling.is_some_and(|ceiling| !dir.starts_with(ceiling)) {
            break;
        }
        if markers.vault_root.is_none() && dir.join(OBSIDIAN_DIR).is_dir() {
            markers.vault_root = Some(dir.to_path_buf());
        }
        if markers.repo_root.is_none() && dir.join(GIT_MARKER).exists() {
            markers.repo_root = Some(dir.to_path_buf());
        }
        if markers.vault_root.is_some() && markers.repo_root.is_some() {
            break;
        }
    }
    markers
}

/// Reads "Strict line breaks" from `<vault>/.obsidian/app.json`. Obsidian's default is `false`.
fn read_strict_line_breaks(vault_root: &Path) -> bool {
    fs::read_to_string(vault_root.join(OBSIDIAN_DIR).join("app.json"))
        .ok()
        .and_then(|json| json_bool(&json, "strictLineBreaks"))
        .unwrap_or(false)
}

/// Finds a top-level boolean value in simple JSON, without a JSON parser.
///
/// This is sufficient for Obsidian's `app.json`, which is a flat object.
fn json_bool(json: &str, key: &str) -> Option<bool> {
    let quoted_key = format!("\"{key}\"");
    let after_key = &json[json.find(&quoted_key)? + quoted_key.len()..];
    let value = after_key.trim_start().strip_prefix(':')?.trim_start();
    if value.starts_with("true") {
        Some(true)
    } else if value.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Removes fenced code blocks and inline code spans from Markdown text.
///
/// This is a simple line scanner, not a full parser. It is only used to find signals.
fn prose(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut open_fence: Option<(char, usize)> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        match open_fence {
            Some((fence_char, fence_len)) => {
                let closes = fence_run(trimmed)
                    .is_some_and(|(c, n)| c == fence_char && n >= fence_len)
                    && trimmed.trim_start_matches(fence_char).trim().is_empty();
                if closes {
                    open_fence = None;
                }
            }
            None => match fence_run(trimmed) {
                Some(fence) => open_fence = Some(fence),
                None => {
                    out.push_str(&strip_code_spans(line));
                    out.push('\n');
                }
            },
        }
    }
    out
}

/// Gives the fence character and length if the line starts a code fence (``` or ~~~).
fn fence_run(line: &str) -> Option<(char, usize)> {
    let fence_char = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = line.chars().take_while(|c| *c == fence_char).count();
    (len >= 3).then_some((fence_char, len))
}

/// Removes inline code spans from one line. A backtick run without a closing run stays.
fn strip_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let run = rest[start..].bytes().take_while(|b| *b == b'`').count();
        let after = &rest[start + run..];
        match find_backtick_run(after, run) {
            Some(end) => rest = &after[end + run..],
            None => {
                out.push_str(&rest[start..start + run]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Finds the byte position of a backtick run with exactly `len` backticks.
fn find_backtick_run(text: &str, len: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
            if run == len {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn context_in(dir: &TempDir, relative_file: &str) -> Context {
        let path = dir.path().join(relative_file);
        Context::with_ceiling(path, String::new(), Some(dir.path()))
    }

    #[test]
    fn known_vault_is_found_without_an_obsidian_folder() {
        let dir = TempDir::new("ctx-known-vault");
        // The settings folder has another name ("Override config folder").
        fs::create_dir_all(dir.path().join("Vault/config")).unwrap();
        let ctx =
            context_in(&dir, "Vault/notes/a.md").with_known_vaults(&[dir.path().join("Vault")]);
        assert_eq!(ctx.vault_root(), None);
        assert_eq!(ctx.known_vault(), Some(dir.path().join("Vault").as_path()));
        assert_eq!(ctx.obsidian_vault(), ctx.known_vault());
        // No `.obsidian/app.json`: Obsidian's default.
        assert!(!ctx.strict_line_breaks());
    }

    #[test]
    fn the_deeper_vault_wins() {
        let dir = TempDir::new("ctx-deeper-vault");
        fs::create_dir_all(dir.path().join("outer/.obsidian")).unwrap();
        fs::create_dir_all(dir.path().join("outer/inner")).unwrap();
        let inner = dir.path().join("outer/inner");
        let outer = dir.path().join("outer");

        let ctx =
            context_in(&dir, "outer/inner/a.md").with_known_vaults(std::slice::from_ref(&inner));
        assert_eq!(ctx.obsidian_vault(), Some(inner.as_path()));

        let ctx =
            context_in(&dir, "outer/inner/a.md").with_known_vaults(&[dir.path().to_path_buf()]);
        assert_eq!(ctx.obsidian_vault(), Some(outer.as_path()));
    }

    #[test]
    fn line_break_setting_is_read_from_a_known_vault_too() {
        let dir = TempDir::new("ctx-known-vault-strict");
        fs::create_dir_all(dir.path().join("Vault/.obsidian")).unwrap();
        fs::write(
            dir.path().join("Vault/.obsidian/app.json"),
            r#"{"strictLineBreaks":true}"#,
        )
        .unwrap();
        // The ceiling hides the `.obsidian` folder from the folder search.
        let notes = dir.path().join("Vault/notes");
        let ctx = Context::with_ceiling(notes.join("a.md"), String::new(), Some(&notes))
            .with_known_vaults(&[dir.path().join("Vault")]);
        assert_eq!(ctx.vault_root(), None);
        assert!(ctx.strict_line_breaks());
    }

    #[test]
    fn finds_vault_root_in_a_parent_folder() {
        let dir = TempDir::new("ctx-vault");
        fs::create_dir_all(dir.path().join("vault/.obsidian")).unwrap();
        fs::create_dir_all(dir.path().join("vault/notes/deep")).unwrap();

        let ctx = context_in(&dir, "vault/notes/deep/note.md");
        assert_eq!(ctx.vault_root(), Some(dir.path().join("vault").as_path()));
        assert_eq!(ctx.repo_root(), None);
    }

    #[test]
    fn finds_repo_root_with_git_folder_or_git_file() {
        let dir = TempDir::new("ctx-repo");
        fs::create_dir_all(dir.path().join("repo/.git")).unwrap();
        fs::create_dir_all(dir.path().join("worktree/docs")).unwrap();
        fs::write(dir.path().join("worktree/.git"), "gitdir: elsewhere").unwrap();

        let in_repo = context_in(&dir, "repo/README.md");
        assert_eq!(in_repo.repo_root(), Some(dir.path().join("repo").as_path()));

        let in_worktree = context_in(&dir, "worktree/docs/guide.md");
        assert_eq!(
            in_worktree.repo_root(),
            Some(dir.path().join("worktree").as_path())
        );
    }

    #[test]
    fn finds_vault_and_repo_at_different_levels() {
        let dir = TempDir::new("ctx-both");
        fs::create_dir_all(dir.path().join("repo/.git")).unwrap();
        fs::create_dir_all(dir.path().join("repo/vault/.obsidian")).unwrap();

        let ctx = context_in(&dir, "repo/vault/note.md");
        assert_eq!(
            ctx.vault_root(),
            Some(dir.path().join("repo/vault").as_path())
        );
        assert_eq!(ctx.repo_root(), Some(dir.path().join("repo").as_path()));
    }

    #[test]
    fn a_file_named_obsidian_is_not_a_vault() {
        let dir = TempDir::new("ctx-not-vault");
        fs::write(dir.path().join(".obsidian"), "").unwrap();

        assert_eq!(context_in(&dir, "note.md").vault_root(), None);
    }

    #[test]
    fn search_stops_at_ceiling() {
        let dir = TempDir::new("ctx-ceiling");
        fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        fs::create_dir_all(dir.path().join("inner")).unwrap();
        let ceiling = dir.path().join("inner");

        let ctx = Context::with_ceiling(ceiling.join("note.md"), String::new(), Some(&ceiling));
        assert_eq!(ctx.vault_root(), None);
    }

    #[test]
    fn reads_strict_line_breaks_from_vault_settings() {
        let dir = TempDir::new("ctx-strict");
        fs::create_dir_all(dir.path().join("strict/.obsidian")).unwrap();
        fs::create_dir_all(dir.path().join("loose/.obsidian")).unwrap();
        fs::create_dir_all(dir.path().join("default/.obsidian")).unwrap();
        fs::write(
            dir.path().join("strict/.obsidian/app.json"),
            "{\n  \"promptDelete\": false,\n  \"strictLineBreaks\" : true\n}",
        )
        .unwrap();
        fs::write(
            dir.path().join("loose/.obsidian/app.json"),
            r#"{"strictLineBreaks":false}"#,
        )
        .unwrap();

        assert!(context_in(&dir, "strict/a.md").strict_line_breaks());
        assert!(!context_in(&dir, "loose/a.md").strict_line_breaks());
        assert!(!context_in(&dir, "default/a.md").strict_line_breaks());
    }

    #[test]
    fn json_bool_reads_true_false_and_rejects_other_values() {
        assert_eq!(json_bool(r#"{"a": true}"#, "a"), Some(true));
        assert_eq!(json_bool("{\"a\"\n:\nfalse}", "a"), Some(false));
        assert_eq!(json_bool(r#"{"a": "true"}"#, "a"), None);
        assert_eq!(json_bool(r#"{"b": true}"#, "a"), None);
        assert_eq!(json_bool("", "a"), None);
    }

    #[test]
    fn load_reads_file_removes_bom_and_repairs_invalid_utf8() {
        let dir = TempDir::new("ctx-load");
        let path = dir.path().join("note.md");
        fs::write(&path, b"\xEF\xBB\xBF# Title \xFF\n").unwrap();

        let ctx = Context::load_in(&path, Some(dir.path()), &[]).unwrap();
        assert_eq!(ctx.text(), "# Title \u{FFFD}\n");
        assert_eq!(ctx.file_name(), Some("note.md"));
        assert!(ctx.path().is_absolute());
    }

    #[test]
    fn load_in_stops_at_the_ceiling_and_uses_the_given_vaults() {
        let dir = TempDir::new("ctx-load-in");
        let outer = dir.path().join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(outer.join(".obsidian")).unwrap();
        fs::create_dir_all(&inner).unwrap();
        let path = inner.join("a.md");
        fs::write(&path, "x").unwrap();

        // The `.obsidian` folder is above the ceiling: the search does not see it.
        let ctx = Context::load_in(&path, Some(&inner), &[]).unwrap();
        assert_eq!(ctx.vault_root(), None);
        assert_eq!(ctx.obsidian_vault(), None);

        // The given vault list is used in place of Obsidian's list.
        let ctx = Context::load_in(&path, Some(&inner), std::slice::from_ref(&outer)).unwrap();
        assert_eq!(ctx.known_vault(), Some(outer.as_path()));

        // Without a ceiling, the search finds the `.obsidian` folder.
        let ctx = Context::load_in(&path, None, &[]).unwrap();
        assert_eq!(ctx.vault_root(), Some(outer.as_path()));
    }

    #[test]
    fn load_reports_missing_file() {
        let dir = TempDir::new("ctx-missing");
        assert!(Context::load(&dir.path().join("missing.md")).is_err());
    }

    #[test]
    fn prose_removes_fenced_code_blocks() {
        let text = "before\n```rust\n[[inside]]\n```\nbetween\n~~~~\n==x==\n~~~\nstill code\n~~~~\nafter\n";
        assert_eq!(prose(text), "before\nbetween\nafter\n");
    }

    #[test]
    fn prose_removes_everything_after_an_unclosed_fence() {
        assert_eq!(prose("text\n```\n[[code]]\n"), "text\n");
    }

    #[test]
    fn prose_removes_inline_code_spans() {
        assert_eq!(prose("a `[[x]]` b ``c ` d`` e\n"), "a  b  e\n");
    }

    #[test]
    fn prose_keeps_unmatched_backticks() {
        assert_eq!(prose("a ` b\n"), "a ` b\n");
        assert_eq!(prose("a `` b ` c\n"), "a `` b ` c\n");
    }
}
