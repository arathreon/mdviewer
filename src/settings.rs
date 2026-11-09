//! User settings, saved as `key = value` lines in `settings.ini`.
//!
//! The format is simple on purpose: no extra crate is necessary, and the user can
//! edit the file in a text editor. Unknown keys and values are ignored with a warning.

use std::fs;
use std::io;
use std::path::Path;

/// A setting with a fixed list of values, shown as a selector in the toolbar.
pub trait Choice: Copy + PartialEq + Sized + 'static {
    /// All values, in the order of the menu.
    const ALL: &'static [Self];
    /// The fixed code in the settings file and in page messages.
    fn id(self) -> &'static str;
    /// The name that the user sees.
    fn name(self) -> &'static str;

    /// Finds a value by its [`id`](Self::id).
    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|choice| choice.id() == id)
    }
}

/// The color theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Follow the Windows setting.
    #[default]
    Auto,
    /// Always the light theme.
    Light,
    /// Always the dark theme.
    Dark,
}

impl Choice for ThemeChoice {
    const ALL: &'static [Self] = &[ThemeChoice::Auto, ThemeChoice::Light, ThemeChoice::Dark];

    fn id(self) -> &'static str {
        match self {
            ThemeChoice::Auto => "auto",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        }
    }

    fn name(self) -> &'static str {
        match self {
            ThemeChoice::Auto => "Auto",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }
}

/// Whether the page loads images and media from the internet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemoteImages {
    /// Load them (for example badges in a README). The servers see the IP address.
    #[default]
    Show,
    /// Do not send any request to the internet.
    Block,
}

impl Choice for RemoteImages {
    const ALL: &'static [Self] = &[RemoteImages::Show, RemoteImages::Block];

    fn id(self) -> &'static str {
        match self {
            RemoteImages::Show => "show",
            RemoteImages::Block => "block",
        }
    }

    fn name(self) -> &'static str {
        match self {
            RemoteImages::Show => "Show",
            RemoteImages::Block => "Block",
        }
    }
}

/// Whether the history panel (the files of this session) shows at the left side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryPanel {
    /// The panel is closed. A new session has only one file, so this is the default.
    #[default]
    Hide,
    /// The panel is open.
    Show,
}

impl Choice for HistoryPanel {
    const ALL: &'static [Self] = &[HistoryPanel::Hide, HistoryPanel::Show];

    fn id(self) -> &'static str {
        match self {
            HistoryPanel::Hide => "hide",
            HistoryPanel::Show => "show",
        }
    }

    fn name(self) -> &'static str {
        match self {
            HistoryPanel::Hide => "Hide",
            HistoryPanel::Show => "Show",
        }
    }
}

/// Whether the window opens maximized. The viewer saves it when the window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowState {
    /// A normal window at the center of the screen.
    #[default]
    Normal,
    /// A maximized window.
    Maximized,
}

impl Choice for WindowState {
    const ALL: &'static [Self] = &[WindowState::Normal, WindowState::Maximized];

    fn id(self) -> &'static str {
        match self {
            WindowState::Normal => "normal",
            WindowState::Maximized => "maximized",
        }
    }

    fn name(self) -> &'static str {
        match self {
            WindowState::Normal => "Normal",
            WindowState::Maximized => "Maximized",
        }
    }
}

impl WindowState {
    /// The state to save when the window closes. A minimized window does not show
    /// whether it was maximized before, so then the old state is kept.
    pub fn when_closed(old: WindowState, maximized: bool, minimized: bool) -> WindowState {
        match (minimized, maximized) {
            (true, _) => old,
            (false, true) => WindowState::Maximized,
            (false, false) => WindowState::Normal,
        }
    }
}

/// The width of the history panel in CSS pixels. The value is always between
/// [`MIN`](Self::MIN) and [`MAX`](Self::MAX), also when the settings file has another
/// number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelWidth(u32);

impl PanelWidth {
    /// The smallest width: the file names must stay readable.
    pub const MIN: u32 = 160;
    /// The largest width. The page script also keeps 200 px for the page.
    pub const MAX: u32 = 800;
    /// The width of a new installation, and after a double-click on the panel edge.
    pub const DEFAULT: u32 = 260;

    /// A width, moved into the allowed range.
    pub fn new(pixels: u32) -> PanelWidth {
        PanelWidth(pixels.clamp(Self::MIN, Self::MAX))
    }

    /// Reads a width from text (a whole number of pixels).
    pub fn parse(text: &str) -> Option<PanelWidth> {
        text.parse().ok().map(PanelWidth::new)
    }

    /// The width in CSS pixels.
    pub fn pixels(self) -> u32 {
        self.0
    }
}

impl Default for PanelWidth {
    fn default() -> PanelWidth {
        PanelWidth(Self::DEFAULT)
    }
}

/// All user settings. They are saved in `%APPDATA%\mdviewer\settings.ini`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// The color theme ("Theme" in the toolbar).
    pub theme: ThemeChoice,
    /// Whether images and media load from the internet ("Remote images" in the toolbar).
    pub remote_images: RemoteImages,
    /// Whether the history panel is open (the history button or Ctrl+H). Only the open
    /// or closed state is saved; the history itself is never saved.
    pub history_panel: HistoryPanel,
    /// The width of the history panel (the user drags its right edge).
    pub history_panel_width: PanelWidth,
    /// Whether the window opens maximized (saved when the window closes).
    pub window: WindowState,
}

impl Settings {
    /// Reads the settings file. A missing or unreadable file gives the default settings.
    pub fn load(path: &Path) -> Settings {
        match fs::read_to_string(path) {
            Ok(text) => Settings::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(error) => {
                log::warn!("cannot read {}: {error}", path.display());
                Settings::default()
            }
        }
    }

    /// Reads settings from text. Empty lines and lines that start with `#` are ignored.
    pub fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                log::warn!("settings: line without '=' ignored: {line:?}");
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "theme" => set(&mut settings.theme, key, value),
                "remote_images" => set(&mut settings.remote_images, key, value),
                "history_panel" => set(&mut settings.history_panel, key, value),
                "history_panel_width" => match PanelWidth::parse(value) {
                    Some(width) => settings.history_panel_width = width,
                    None => log::warn!("settings: unknown value {value:?} for {key:?} ignored"),
                },
                "window" => set(&mut settings.window, key, value),
                _ => log::warn!("settings: unknown key {key:?} ignored"),
            }
        }
        settings
    }

    /// Gives the text of the settings file.
    pub fn to_text(&self) -> String {
        format!(
            "# mdviewer settings\n\
             # theme: auto, light or dark\n\
             theme = {}\n\
             # remote_images: show or block (block: no requests to the internet)\n\
             remote_images = {}\n\
             # history_panel: show or hide (the tree of the files of this session)\n\
             history_panel = {}\n\
             # history_panel_width: the width of the history panel in pixels (160 to 800)\n\
             history_panel_width = {}\n\
             # window: normal or maximized (saved when the window closes)\n\
             window = {}\n",
            self.theme.id(),
            self.remote_images.id(),
            self.history_panel.id(),
            self.history_panel_width.pixels(),
            self.window.id()
        )
    }

    /// Writes the settings file. The text goes to a temporary file first, which then
    /// replaces the old file, so that a crash cannot leave a half-written file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temporary = path.with_extension("ini.tmp");
        fs::write(&temporary, self.to_text())?;
        fs::rename(&temporary, path)
    }
}

/// Sets a choice from its id, or keeps the old value and writes a warning.
fn set<C: Choice>(target: &mut C, key: &str, value: &str) {
    match C::from_id(value) {
        Some(choice) => *target = choice,
        None => log::warn!("settings: unknown value {value:?} for {key:?} ignored"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn ids_go_both_ways<C: Choice + std::fmt::Debug>() {
        for &choice in C::ALL {
            assert_eq!(C::from_id(choice.id()), Some(choice));
        }
        assert_eq!(C::from_id(""), None);
    }

    #[test]
    fn choice_ids_go_both_ways_and_are_case_sensitive() {
        ids_go_both_ways::<ThemeChoice>();
        ids_go_both_ways::<RemoteImages>();
        ids_go_both_ways::<HistoryPanel>();
        ids_go_both_ways::<WindowState>();
        assert_eq!(ThemeChoice::from_id("Dark"), None);
        assert_eq!(RemoteImages::from_id("BLOCK"), None);
    }

    #[test]
    fn defaults_are_auto_theme_shown_images_and_closed_panel() {
        let settings = Settings::default();
        assert_eq!(settings.theme, ThemeChoice::Auto);
        assert_eq!(settings.remote_images, RemoteImages::Show);
        assert_eq!(settings.history_panel, HistoryPanel::Hide);
        assert_eq!(settings.history_panel_width.pixels(), 260);
        assert_eq!(settings.window, WindowState::Normal);
    }

    #[test]
    fn parse_reads_values_with_spaces_and_comments() {
        let text = "# comment\n\n  theme =  dark  \nremote_images=block\nhistory_panel = show\n\
                    history_panel_width = 333\nwindow = maximized\n";
        assert_eq!(
            Settings::parse(text),
            Settings {
                theme: ThemeChoice::Dark,
                remote_images: RemoteImages::Block,
                history_panel: HistoryPanel::Show,
                history_panel_width: PanelWidth::new(333),
                window: WindowState::Maximized,
            }
        );
    }

    #[test]
    fn parse_ignores_bad_lines_and_unknown_values() {
        let text = "theme = purple\nremote_images = maybe\nhistory_panel = 1\nno equals sign\ncolor = red\n\
                    history_panel_width = wide\nhistory_panel_width = -5\nhistory_panel_width = 2.5\n\
                    window = Maximized\nwindow = full\n";
        assert_eq!(Settings::parse(text), Settings::default());
    }

    #[test]
    fn window_state_when_closed_keeps_the_old_state_for_a_minimized_window() {
        use WindowState::{Maximized, Normal};
        for old in [Normal, Maximized] {
            assert_eq!(WindowState::when_closed(old, true, false), Maximized);
            assert_eq!(WindowState::when_closed(old, false, false), Normal);
            // Minimized: Windows does not say whether it was maximized before.
            assert_eq!(WindowState::when_closed(old, false, true), old);
            assert_eq!(WindowState::when_closed(old, true, true), old);
        }
    }

    #[test]
    fn panel_width_stays_in_its_range() {
        assert_eq!(PanelWidth::new(0).pixels(), PanelWidth::MIN);
        assert_eq!(PanelWidth::new(5000).pixels(), PanelWidth::MAX);
        assert_eq!(PanelWidth::new(300).pixels(), 300);
        // A number from an edited settings file is moved into the range, not ignored.
        assert_eq!(
            Settings::parse("history_panel_width = 99999\n").history_panel_width,
            PanelWidth::new(PanelWidth::MAX)
        );
        assert_eq!(PanelWidth::parse(" 300"), None);
        assert_eq!(PanelWidth::parse("300"), Some(PanelWidth::new(300)));
    }

    #[test]
    fn last_value_wins() {
        assert_eq!(
            Settings::parse("theme = dark\ntheme = light\n").theme,
            ThemeChoice::Light
        );
    }

    #[test]
    fn text_can_be_read_again() {
        for &theme in ThemeChoice::ALL {
            for &remote_images in RemoteImages::ALL {
                for &history_panel in HistoryPanel::ALL {
                    let settings = Settings {
                        theme,
                        remote_images,
                        history_panel,
                        history_panel_width: PanelWidth::new(420),
                        window: WindowState::Maximized,
                    };
                    assert_eq!(Settings::parse(&settings.to_text()), settings);
                }
            }
        }
    }

    #[test]
    fn save_creates_folder_and_load_reads_it() {
        let dir = TempDir::new("settings-save");
        let path = dir.path().join("sub").join("settings.ini");
        let settings = Settings {
            theme: ThemeChoice::Light,
            remote_images: RemoteImages::Block,
            history_panel: HistoryPanel::Show,
            history_panel_width: PanelWidth::new(500),
            window: WindowState::Maximized,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        assert!(!path.with_extension("ini.tmp").exists());

        let changed = Settings {
            theme: ThemeChoice::Dark,
            ..settings
        };
        changed.save(&path).unwrap();
        assert_eq!(Settings::load(&path), changed);
    }

    #[test]
    fn load_of_missing_file_gives_default() {
        let dir = TempDir::new("settings-missing");
        assert_eq!(
            Settings::load(&dir.path().join("settings.ini")),
            Settings::default()
        );
    }
}
