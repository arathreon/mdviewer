//! The vaults that Obsidian knows on this computer.
//!
//! Obsidian keeps a list of all vaults that the user opened in `obsidian/obsidian.json`
//! below the configuration folder of the user (on Windows `%APPDATA%`, see
//! [`platform::config_base`]):
//!
//! ```json
//! {"vaults":{"0123abcd":{"path":"D:\\Notes\\Example","ts":1730000000000,"open":true}}}
//! ```
//!
//! This finds a vault also when its settings folder is not `.obsidian` (Obsidian has a
//! setting "Override config folder"), and it is Obsidian's own record, not a guess.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::platform;

/// `<base>/obsidian/obsidian.json`, if `config_base` is an absolute folder.
pub fn registry_file(config_base: Option<OsString>) -> Option<PathBuf> {
    let base = PathBuf::from(config_base?);
    base.is_absolute()
        .then(|| base.join("obsidian").join("obsidian.json"))
}

/// The known vaults of this user. An empty list if Obsidian is not installed.
pub fn load_from_env() -> Vec<PathBuf> {
    registry_file(platform::config_base())
        .map(|file| load(&file))
        .unwrap_or_default()
}

/// Reads the vault list from `obsidian.json`. A missing file gives an empty list.
pub fn load(file: &Path) -> Vec<PathBuf> {
    match fs::read_to_string(file) {
        Ok(json) => parse(&json),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            log::warn!("cannot read {}: {error}", file.display());
            Vec::new()
        }
    }
}

/// The values of all `"path"` keys. Only vault entries have this key.
pub fn parse(json: &str) -> Vec<PathBuf> {
    string_values(json, "path")
        .into_iter()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// The deepest vault that contains `file`. Names are compared with the rules of the file
/// system ([`platform::same_name`]; on Windows, letter case is ignored).
pub fn containing<'a>(vaults: &'a [PathBuf], file: &Path) -> Option<&'a Path> {
    vaults
        .iter()
        .filter(|vault| crate::paths::is_inside(file, vault))
        .max_by_key(|vault| vault.components().count())
        .map(PathBuf::as_path)
}

/// Finds the string values of `"key": "..."` in JSON text, without a JSON parser.
pub(crate) fn string_values(json: &str, key: &str) -> Vec<String> {
    let quoted_key = format!("\"{key}\"");
    let mut values = Vec::new();
    let mut rest = json;
    while let Some(start) = rest.find(&quoted_key) {
        rest = &rest[start + quoted_key.len()..];
        let Some(after_colon) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        let Some(string) = after_colon.trim_start().strip_prefix('"') else {
            continue;
        };
        if let Some((value, length)) = read_string(string) {
            values.push(value);
            rest = &string[length..];
        }
    }
    values
}

/// Reads a JSON string after its opening quote. Gives the value and the length that was
/// read (with the closing quote), or `None` if the string is not complete or not valid.
fn read_string(text: &str) -> Option<(String, usize)> {
    let mut value = String::new();
    let mut chars = text.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((value, index + 1)),
            '\\' => {
                let (_, escaped) = chars.next()?;
                match escaped {
                    '"' | '\\' | '/' => value.push(escaped),
                    'b' => value.push('\u{8}'),
                    'f' => value.push('\u{c}'),
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'u' => {
                        let first = read_hex4(&mut chars)?;
                        let code = if (0xD800..0xDC00).contains(&first) {
                            // A surrogate pair: \uD83D\uDE00.
                            let (_, backslash) = chars.next()?;
                            let (_, u) = chars.next()?;
                            if backslash != '\\' || u != 'u' {
                                return None;
                            }
                            let second = read_hex4(&mut chars)?;
                            if !(0xDC00..0xE000).contains(&second) {
                                return None;
                            }
                            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                        } else {
                            first
                        };
                        value.push(char::from_u32(code)?);
                    }
                    _ => return None,
                }
            }
            _ => value.push(c),
        }
    }
    None
}

/// Reads the 4 hexadecimal digits of a JSON `\u` escape.
fn read_hex4(chars: &mut std::str::CharIndices<'_>) -> Option<u32> {
    let mut code = 0;
    for _ in 0..4 {
        let (_, digit) = chars.next()?;
        code = code * 16 + digit.to_digit(16)?;
    }
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    #[test]
    fn registry_file_is_below_app_data() {
        let base = std::env::temp_dir();
        assert_eq!(
            registry_file(Some(base.clone().into_os_string())),
            Some(base.join("obsidian").join("obsidian.json"))
        );
        assert_eq!(registry_file(None), None);
        assert_eq!(registry_file(Some(OsString::from("relative"))), None);
    }

    #[test]
    fn parse_reads_all_vault_paths_with_escapes() {
        let json = r#"{"vaults":{
            "a1":{"path":"D:\\Data\\obsidian\\Example","ts":1730000000000,"open":true},
            "b2":{"path" : "C:\\Users\\me\\OneDrive\\Notes \u00e9t\u00e9","ts":1},
            "c3":{"path":"E:\\Emoji \ud83d\ude00\/x"}
        },"frame":"hidden"}"#;
        assert_eq!(
            parse(json),
            [
                PathBuf::from(r"D:\Data\obsidian\Example"),
                PathBuf::from(r"C:\Users\me\OneDrive\Notes été"),
                PathBuf::from(r"E:\Emoji 😀/x"),
            ]
        );
    }

    #[test]
    fn parse_skips_bad_and_empty_values() {
        assert!(parse("").is_empty());
        assert!(parse("not json").is_empty());
        assert!(parse(r#"{"path": 12}"#).is_empty());
        assert!(parse(r#"{"path": ""}"#).is_empty());
        assert!(parse(r#"{"path": "unclosed"#).is_empty());
        assert!(parse(r#"{"path": "bad \q escape"}"#).is_empty());
        assert!(parse(r#"{"path": "lone \ud83d surrogate"}"#).is_empty());
        // A good value after a bad one is still found.
        assert_eq!(
            parse(r#"{"path": 1, "x": {"path": "C:\\ok"}}"#),
            [PathBuf::from(r"C:\ok")]
        );
    }

    #[test]
    fn escaped_quotes_in_values_do_not_start_a_new_key() {
        let json = r#"{"name": "a \"path\": \"C:\\fake\"", "path": "C:\\real"}"#;
        assert_eq!(parse(json), [PathBuf::from(r"C:\real")]);
    }

    #[test]
    #[cfg(windows)]
    fn containing_finds_the_deepest_vault_and_ignores_case() {
        let vaults = [
            PathBuf::from(r"D:\Data\obsidian"),
            PathBuf::from(r"D:\Data\obsidian\Example"),
            PathBuf::from(r"C:\Other"),
        ];
        let file = Path::new(r"d:\DATA\Obsidian\example\Notes\a.md");
        assert_eq!(
            containing(&vaults, file),
            Some(Path::new(r"D:\Data\obsidian\Example"))
        );
        assert_eq!(
            containing(&vaults, Path::new(r"D:\Data\obsidian\x.md")),
            Some(Path::new(r"D:\Data\obsidian"))
        );
    }

    #[test]
    fn containing_compares_whole_folder_names() {
        let vaults = [PathBuf::from(r"D:\Notes")];
        assert_eq!(containing(&vaults, Path::new(r"D:\Notes2\a.md")), None);
        assert_eq!(containing(&vaults, Path::new(r"D:\a.md")), None);
        assert_eq!(containing(&vaults, Path::new(r"E:\Notes\a.md")), None);
        assert_eq!(containing(&[], Path::new(r"D:\Notes\a.md")), None);
        // A trailing separator in the vault path does not matter.
        let with_slash = [PathBuf::from(r"D:\Notes\")];
        assert!(containing(&with_slash, Path::new(r"D:\Notes\a.md")).is_some());
    }

    #[test]
    fn load_reads_a_file_and_accepts_a_missing_file() {
        let dir = TempDir::new("obsidian-registry");
        let file = dir.path().join("obsidian.json");
        assert!(load(&file).is_empty());
        fs::write(&file, r#"{"vaults":{"x":{"path":"C:\\Vault"}}}"#).unwrap();
        assert_eq!(load(&file), [PathBuf::from(r"C:\Vault")]);
    }
}
