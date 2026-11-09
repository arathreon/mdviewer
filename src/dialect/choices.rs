//! The dialect that the user chose by hand ("Dialect" in the toolbar), for each folder.
//!
//! The automatic detection can be wrong, so the user can choose the dialect. The choice
//! is saved for the folder of the document and is also valid for its subfolders. The
//! nearest saved folder wins: Obsidian for `D:\Notes` and GitHub for `D:\Notes\Code`
//! give GitHub for `D:\Notes\Code\x\a.md` and Obsidian for `D:\Notes\Daily\b.md`.
//!
//! "Auto" is also a choice: it removes the entry of the folder, and if a parent folder
//! still has a dialect, it saves `auto`, so that detection really decides there.
//!
//! The file (`dialects.ini`, see `paths.rs`) has one line for each folder:
//!
//! ```text
//! D:\Notes = obsidian
//! D:\Notes\Code = auto
//! ```
//!
//! A line is split at the last `=`: a folder name can contain `=`, a dialect id cannot.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{DIALECTS, Detection, Dialect};
use crate::paths::is_inside;

/// The largest number of saved folders. When there are more, the oldest choice is
/// removed, so that the file cannot grow without limit.
pub const MAX_FOLDERS: usize = 1000;

/// The id of the automatic detection in the settings file and in page messages.
pub const AUTO_ID: &str = "auto";

/// One entry of the "Dialect" selector: the automatic detection, or one dialect.
#[derive(Clone, Copy)]
pub enum DialectChoice {
    /// The detection decides.
    Auto,
    /// Always this dialect.
    Fixed(&'static Dialect),
}

impl DialectChoice {
    /// The fixed code in the file and in page messages: `auto` or the dialect id.
    pub fn id(self) -> &'static str {
        match self {
            DialectChoice::Auto => AUTO_ID,
            DialectChoice::Fixed(dialect) => dialect.id,
        }
    }

    /// Finds a choice by its [`id`](Self::id). Unknown ids give `None`.
    pub fn from_id(id: &str) -> Option<DialectChoice> {
        if id == AUTO_ID {
            Some(DialectChoice::Auto)
        } else {
            super::find(id).map(DialectChoice::Fixed)
        }
    }

    /// All choices in the order of the selector: "Auto", then each known dialect.
    pub fn all() -> impl Iterator<Item = DialectChoice> {
        std::iter::once(DialectChoice::Auto).chain(DIALECTS.iter().map(DialectChoice::Fixed))
    }
}

impl PartialEq for DialectChoice {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for DialectChoice {}

impl fmt::Debug for DialectChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Where the choice for a document comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The folder of the document has the choice.
    ThisFolder,
    /// A parent folder has the choice.
    ParentFolder,
}

/// The dialect of one document: what the detection found, and the choice of the user.
#[derive(Debug)]
pub struct Selection {
    /// The result of the automatic detection (always done, for the "Auto" entry).
    pub detection: Detection,
    /// The saved choice for the folder of the document, if there is one.
    pub saved: Option<(DialectChoice, Origin)>,
}

impl Selection {
    /// The dialect that renders the document: the chosen one, or the detected one.
    pub fn dialect(&self) -> &'static Dialect {
        match self.saved {
            Some((DialectChoice::Fixed(dialect), _)) => dialect,
            _ => self.detection.dialect,
        }
    }

    /// The selected entry of the selector.
    pub fn choice(&self) -> DialectChoice {
        self.saved.map_or(DialectChoice::Auto, |(choice, _)| choice)
    }

    /// A short explanation for the log, for example
    /// `"Obsidian, chosen for this folder (detected: GitHub, no rule matched (default))"`.
    pub fn describe(&self) -> String {
        let detected = format!(
            "{}, {}",
            self.detection.dialect.name,
            self.detection.reasons.join(", ")
        );
        match self.saved {
            Some((DialectChoice::Fixed(dialect), origin)) => format!(
                "{}, chosen for {} (detected: {detected})",
                dialect.name,
                match origin {
                    Origin::ThisFolder => "this folder",
                    Origin::ParentFolder => "a parent folder",
                }
            ),
            _ => detected,
        }
    }
}

/// The saved choices: a list of (folder, choice), the oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderChoices {
    /// Absolute folders and their choices. Each folder is in the list only once
    /// (after [`set`](Self::set)); in a file, a later line wins.
    entries: Vec<(PathBuf, DialectChoice)>,
}

impl FolderChoices {
    /// Reads the file. A missing or unreadable file gives an empty list.
    pub fn load(path: &Path) -> FolderChoices {
        match fs::read_to_string(path) {
            Ok(text) => FolderChoices::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => FolderChoices::default(),
            Err(error) => {
                log::warn!("cannot read {}: {error}", path.display());
                FolderChoices::default()
            }
        }
    }

    /// Reads the choices from text. Empty lines and lines that start with `#` are
    /// ignored. Lines with a relative folder or an unknown dialect are ignored with a
    /// warning.
    pub fn parse(text: &str) -> FolderChoices {
        let mut choices = FolderChoices::default();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((folder, id)) = line.rsplit_once('=') else {
                log::warn!("dialects: line without '=' ignored: {line:?}");
                continue;
            };
            let folder = PathBuf::from(folder.trim());
            if !folder.is_absolute() {
                log::warn!("dialects: folder is not absolute, ignored: {line:?}");
                continue;
            }
            match DialectChoice::from_id(id.trim()) {
                Some(choice) => choices.entries.push((folder, choice)),
                None => log::warn!("dialects: unknown dialect, ignored: {line:?}"),
            }
        }
        choices
    }

    /// Gives the text of the file.
    pub fn to_text(&self) -> String {
        let ids: Vec<_> = DialectChoice::all().map(DialectChoice::id).collect();
        let mut text = format!(
            "# mdviewer: the Markdown dialect chosen for each folder (\"Dialect\" in the toolbar).\n\
             # A choice is also valid for the subfolders; the nearest folder wins.\n\
             # <folder> = {}\n",
            ids.join(", ")
        );
        for (folder, choice) in &self.entries {
            text.push_str(&format!("{} = {}\n", folder.display(), choice.id()));
        }
        text
    }

    /// Writes the file: first a temporary file, then a rename, so that a crash cannot
    /// leave half a file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temporary = path.with_extension("ini.tmp");
        fs::write(&temporary, self.to_text())?;
        fs::rename(&temporary, path)
    }

    /// The choice for documents in `folder`: the entry of the nearest folder that
    /// contains it (the folder itself or a parent), or `None`.
    pub fn for_folder(&self, folder: &Path) -> Option<(DialectChoice, Origin)> {
        let depth = |path: &Path| path.components().count();
        // `max_by_key` gives the last of equal elements, so a later line wins.
        let (nearest, choice) = self
            .entries
            .iter()
            .filter(|(saved, _)| is_inside(folder, saved))
            .max_by_key(|(saved, _)| depth(saved))?;
        let origin = if depth(nearest) == depth(folder) {
            Origin::ThisFolder
        } else {
            Origin::ParentFolder
        };
        Some((*choice, origin))
    }

    /// Saves `choice` for `folder` (and its subfolders). The entry moves to the end of
    /// the list (the newest). Returns `false` if the folder cannot be saved in the file
    /// (a relative path, a name that is not Unicode, or a line break in the name).
    pub fn set(&mut self, folder: &Path, choice: DialectChoice) -> bool {
        let Some(text) = folder.to_str() else {
            return false;
        };
        if !folder.is_absolute() || text.contains(['\n', '\r']) {
            return false;
        }
        self.entries
            .retain(|(saved, _)| !(is_inside(saved, folder) && is_inside(folder, saved)));
        // "Auto" needs an entry only when a parent folder has another choice.
        let inherited = self
            .for_folder(folder)
            .map_or(DialectChoice::Auto, |(choice, _)| choice);
        if choice != DialectChoice::Auto || inherited != DialectChoice::Auto {
            self.entries.push((folder.to_path_buf(), choice));
        }
        if self.entries.len() > MAX_FOLDERS {
            let extra = self.entries.len() - MAX_FOLDERS;
            self.entries.drain(..extra);
        }
        true
    }

    /// The number of saved folders.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Gives `true` if no folder has a choice.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{DEFAULT_REASON, github, obsidian};
    use crate::test_util::TempDir;

    const GITHUB: DialectChoice = DialectChoice::Fixed(&github::DIALECT);
    const OBSIDIAN: DialectChoice = DialectChoice::Fixed(&obsidian::DIALECT);

    fn path(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn choices_have_fixed_ids_and_auto_comes_first() {
        let ids: Vec<_> = DialectChoice::all().map(DialectChoice::id).collect();
        assert_eq!(ids, ["auto", "github", "obsidian"]);
        for choice in DialectChoice::all() {
            assert_eq!(DialectChoice::from_id(choice.id()), Some(choice));
        }
        assert_eq!(DialectChoice::from_id("Obsidian"), None);
        assert_eq!(DialectChoice::from_id(""), None);
    }

    #[test]
    fn the_nearest_folder_wins() {
        let mut choices = FolderChoices::default();
        assert!(choices.set(&path(r"D:\Notes"), OBSIDIAN));
        assert!(choices.set(&path(r"D:\Notes\Code"), GITHUB));
        assert_eq!(
            choices.for_folder(&path(r"D:\Notes")),
            Some((OBSIDIAN, Origin::ThisFolder))
        );
        assert_eq!(
            choices.for_folder(&path(r"D:\Notes\Daily")),
            Some((OBSIDIAN, Origin::ParentFolder))
        );
        assert_eq!(
            choices.for_folder(&path(r"D:\Notes\Code\x")),
            Some((GITHUB, Origin::ParentFolder))
        );
        assert_eq!(choices.for_folder(&path(r"D:\Other")), None);
        assert_eq!(choices.for_folder(&path(r"D:\Notes2")), None);
    }

    #[test]
    fn a_new_choice_replaces_the_old_one_of_the_same_folder() {
        let mut choices = FolderChoices::default();
        choices.set(&path(r"D:\Notes"), OBSIDIAN);
        choices.set(&path(r"D:\Notes"), GITHUB);
        assert_eq!(choices.len(), 1);
        assert_eq!(
            choices.for_folder(&path(r"D:\Notes")),
            Some((GITHUB, Origin::ThisFolder))
        );
    }

    #[test]
    #[cfg(windows)]
    fn folders_ignore_letter_case_on_windows() {
        let mut choices = FolderChoices::default();
        choices.set(&path(r"D:\Notes"), OBSIDIAN);
        choices.set(&path(r"d:\NOTES"), GITHUB);
        assert_eq!(choices.len(), 1);
        assert_eq!(
            choices.for_folder(&path(r"d:\notes\daily")),
            Some((GITHUB, Origin::ParentFolder))
        );
    }

    #[test]
    fn auto_removes_the_entry_when_no_parent_decides() {
        let mut choices = FolderChoices::default();
        choices.set(&path(r"D:\Notes"), OBSIDIAN);
        choices.set(&path(r"D:\Notes"), DialectChoice::Auto);
        assert!(choices.is_empty());
        // "Auto" for a folder without any choice saves nothing.
        choices.set(&path(r"D:\Other"), DialectChoice::Auto);
        assert!(choices.is_empty());
    }

    #[test]
    fn auto_below_a_chosen_parent_is_saved_so_that_detection_decides() {
        let mut choices = FolderChoices::default();
        choices.set(&path(r"D:\Notes"), OBSIDIAN);
        choices.set(&path(r"D:\Notes\Code"), DialectChoice::Auto);
        assert_eq!(
            choices.for_folder(&path(r"D:\Notes\Code\x")),
            Some((DialectChoice::Auto, Origin::ParentFolder))
        );
        assert_eq!(
            choices.for_folder(&path(r"D:\Notes\Daily")),
            Some((OBSIDIAN, Origin::ParentFolder))
        );
    }

    #[test]
    fn folders_that_cannot_be_saved_are_refused() {
        let mut choices = FolderChoices::default();
        assert!(!choices.set(&path(r"relative\folder"), OBSIDIAN));
        assert!(!choices.set(&path("D:\\Notes\nD:\\Evil = github"), OBSIDIAN));
        assert!(choices.is_empty());
    }

    #[test]
    fn the_limit_removes_the_oldest_choices() {
        let mut choices = FolderChoices::default();
        for number in 0..MAX_FOLDERS + 5 {
            choices.set(&path(&format!(r"D:\Notes\{number}")), OBSIDIAN);
        }
        assert_eq!(choices.len(), MAX_FOLDERS);
        assert_eq!(choices.for_folder(&path(r"D:\Notes\4")), None);
        assert!(choices.for_folder(&path(r"D:\Notes\5")).is_some());
        // A changed choice is the newest again.
        choices.set(&path(r"D:\Notes\5"), GITHUB);
        choices.set(&path(r"D:\Notes\new"), GITHUB);
        assert!(choices.for_folder(&path(r"D:\Notes\5")).is_some());
        assert_eq!(choices.for_folder(&path(r"D:\Notes\6")), None);
    }

    #[test]
    fn the_text_is_read_back_and_bad_lines_are_ignored() {
        let mut choices = FolderChoices::default();
        choices.set(&path(r"D:\Notes"), OBSIDIAN);
        choices.set(&path(r"D:\a = b"), GITHUB);
        choices.set(&path(r"D:\Notes\Code"), DialectChoice::Auto);
        let text = choices.to_text();
        assert!(
            text.contains("# <folder> = auto, github, obsidian\n"),
            "{text}"
        );
        assert!(text.contains("D:\\a = b = github\n"), "{text}");
        assert_eq!(FolderChoices::parse(&text), choices);

        let parsed = FolderChoices::parse(
            "# comment\n\n\
             no equals sign\n\
             relative = obsidian\n\
             D:\\x = unknown\n\
             D:\\y = obsidian\n\
             D:\\y = github\n",
        );
        // Only the two lines for D:\y are valid, and the later one wins.
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed.for_folder(&path(r"D:\y")),
            Some((GITHUB, Origin::ThisFolder))
        );
    }

    #[test]
    fn save_creates_the_folder_and_load_reads_it() {
        let dir = TempDir::new("dialect-choices");
        let file = dir.path().join("sub").join("dialects.ini");
        assert!(FolderChoices::load(&file).is_empty());
        let mut choices = FolderChoices::default();
        choices.set(&dir.path().join("Notes"), OBSIDIAN);
        choices.save(&file).unwrap();
        assert_eq!(FolderChoices::load(&file), choices);
        assert!(!file.with_extension("ini.tmp").exists());
    }

    fn detected_github() -> Detection {
        Detection {
            dialect: &github::DIALECT,
            reasons: vec![DEFAULT_REASON],
        }
    }

    #[test]
    fn a_selection_uses_the_chosen_dialect_and_keeps_the_detection() {
        let auto = Selection {
            detection: detected_github(),
            saved: None,
        };
        assert_eq!(auto.dialect().id, "github");
        assert_eq!(auto.choice(), DialectChoice::Auto);
        assert_eq!(auto.describe(), "GitHub, no rule matched (default)");

        let chosen = Selection {
            detection: detected_github(),
            saved: Some((OBSIDIAN, Origin::ParentFolder)),
        };
        assert_eq!(chosen.dialect().id, "obsidian");
        assert_eq!(chosen.choice(), OBSIDIAN);
        assert_eq!(
            chosen.describe(),
            "Obsidian, chosen for a parent folder (detected: GitHub, no rule matched (default))"
        );

        // A saved "auto" (below a chosen parent) means: the detection decides.
        let saved_auto = Selection {
            detection: detected_github(),
            saved: Some((DialectChoice::Auto, Origin::ThisFolder)),
        };
        assert_eq!(saved_auto.dialect().id, "github");
        assert_eq!(saved_auto.choice(), DialectChoice::Auto);
    }
}
