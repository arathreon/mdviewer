//! Markdown dialects and their automatic detection.
//!
//! To add a dialect: write a module with a `DIALECT` constant, and add it to [`DIALECTS`].

pub mod choices;
pub mod github;
pub mod obsidian;

use std::fmt;

use crate::context::Context;
use crate::feature::Feature;

/// All known dialects. The first one is the default when no rule matches.
pub static DIALECTS: &[Dialect] = &[github::DIALECT, obsidian::DIALECT];

/// The reason that is shown when no rule matches.
pub const DEFAULT_REASON: &str = "no rule matched (default)";

/// A Markdown dialect: its detection rules and the syntax features it uses.
///
/// This is plain data, not a trait, because a dialect has no behavior of its own: a new
/// dialect is a new constant with rules and a feature list.
pub struct Dialect {
    /// A short, fixed code, for example `"obsidian"`. Saved settings use it, so it must not change.
    pub id: &'static str,
    /// The name that the user sees, for example `"Obsidian"`.
    pub name: &'static str,
    /// The detection rules.
    pub rules: &'static [Rule],
    /// Gives the syntax features for one document.
    pub features: fn(&Context) -> Vec<&'static dyn Feature>,
}

impl fmt::Debug for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dialect").field("id", &self.id).finish()
    }
}

/// The group of a rule. A match in a higher tier always wins over any lower tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Signals from the folders, for example an `.obsidian` folder. Strong.
    Location,
    /// Signals from the text, for example wiki links. Weak.
    Content,
}

/// The tiers from strongest to weakest.
const TIERS: [Tier; 2] = [Tier::Location, Tier::Content];

/// One detection rule of a dialect.
pub struct Rule {
    /// The group of the rule: a match in a stronger tier always wins.
    pub tier: Tier,
    /// Strength inside the tier. A rule with weight 0 never decides.
    pub weight: u32,
    /// A short text for the user, for example `".obsidian folder found"`.
    pub reason: &'static str,
    /// Gives `true` if the rule matches the document.
    pub test: fn(&Context) -> bool,
}

/// The result of detection.
#[derive(Debug)]
pub struct Detection {
    /// The dialect that won.
    pub dialect: &'static Dialect,
    /// The reasons of the rules that decided.
    pub reasons: Vec<&'static str>,
}

/// Finds the dialect of a document with the rules of all known dialects.
pub fn detect(ctx: &Context) -> Detection {
    detect_in(DIALECTS, ctx)
}

/// Finds the dialect of a document from `dialects`.
///
/// 1. The strongest tier that has a matching rule decides.
/// 2. In that tier, the dialect with the highest total weight wins.
/// 3. If two dialects have the same total, the one that is first in the list wins.
/// 4. If no rule matches, the first dialect wins.
///
/// # Panics
///
/// If `dialects` is empty.
pub fn detect_in(dialects: &'static [Dialect], ctx: &Context) -> Detection {
    for tier in TIERS {
        let mut best: Option<(u32, Detection)> = None;
        for dialect in dialects {
            let matched: Vec<&Rule> = dialect
                .rules
                .iter()
                .filter(|rule| rule.tier == tier && (rule.test)(ctx))
                .collect();
            let weight: u32 = matched.iter().map(|rule| rule.weight).sum();
            if weight > 0
                && best
                    .as_ref()
                    .is_none_or(|(best_weight, _)| weight > *best_weight)
            {
                let reasons = matched.iter().map(|rule| rule.reason).collect();
                best = Some((weight, Detection { dialect, reasons }));
            }
        }
        if let Some((_, detection)) = best {
            return detection;
        }
    }
    Detection {
        dialect: &dialects[0],
        reasons: vec![DEFAULT_REASON],
    }
}

/// Finds a dialect by its id, for example for the manual choice of the user.
pub fn find(id: &str) -> Option<&'static Dialect> {
    DIALECTS.iter().find(|dialect| dialect.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_features(_: &Context) -> Vec<&'static dyn Feature> {
        Vec::new()
    }

    static TEST_DIALECTS: &[Dialect] = &[
        Dialect {
            id: "first",
            name: "First",
            rules: &[
                Rule {
                    tier: Tier::Content,
                    weight: 5,
                    reason: "first: content A",
                    test: |ctx| ctx.text().contains('A'),
                },
                Rule {
                    tier: Tier::Content,
                    weight: 5,
                    reason: "first: content B",
                    test: |ctx| ctx.text().contains('B'),
                },
                Rule {
                    tier: Tier::Content,
                    weight: 0,
                    reason: "first: weight zero",
                    test: |ctx| ctx.text().contains('Z'),
                },
            ],
            features: no_features,
        },
        Dialect {
            id: "second",
            name: "Second",
            rules: &[
                Rule {
                    tier: Tier::Location,
                    weight: 1,
                    reason: "second: location L",
                    test: |ctx| ctx.text().contains('L'),
                },
                Rule {
                    tier: Tier::Content,
                    weight: 8,
                    reason: "second: content C",
                    test: |ctx| ctx.text().contains('C'),
                },
                Rule {
                    tier: Tier::Content,
                    weight: 10,
                    reason: "second: content D",
                    test: |ctx| ctx.text().contains('D'),
                },
            ],
            features: no_features,
        },
    ];

    fn detect_text(text: &str) -> Detection {
        detect_in(TEST_DIALECTS, &Context::detached("test.md", text))
    }

    #[test]
    fn no_match_gives_first_dialect_with_default_reason() {
        let detection = detect_text("nothing");
        assert_eq!(detection.dialect.id, "first");
        assert_eq!(detection.reasons, [DEFAULT_REASON]);
    }

    #[test]
    fn location_tier_wins_over_heavier_content_tier() {
        let detection = detect_text("L A B");
        assert_eq!(detection.dialect.id, "second");
        assert_eq!(detection.reasons, ["second: location L"]);
    }

    #[test]
    fn weights_in_a_tier_are_added() {
        let detection = detect_text("A B C");
        assert_eq!(detection.dialect.id, "first");
        assert_eq!(detection.reasons, ["first: content A", "first: content B"]);
    }

    #[test]
    fn higher_total_weight_wins() {
        assert_eq!(detect_text("A C").dialect.id, "second");
    }

    #[test]
    fn equal_weight_gives_the_dialect_that_is_first_in_the_list() {
        // first: A + B = 10, second: D = 10.
        assert_eq!(detect_text("A B D").dialect.id, "first");
    }

    #[test]
    fn rule_with_weight_zero_never_decides() {
        let detection = detect_text("Z");
        assert_eq!(detection.reasons, [DEFAULT_REASON]);
    }

    #[test]
    fn find_gives_known_dialects_only() {
        assert_eq!(find("github").map(|d| d.name), Some("GitHub"));
        assert_eq!(find("obsidian").map(|d| d.name), Some("Obsidian"));
        assert!(find("unknown").is_none());
    }

    /// Detection with the real dialects and real folders.
    mod real {
        use super::*;
        use crate::test_util::TempDir;
        use std::fs;

        fn detect_file(dir: &TempDir, relative_file: &str, text: &str) -> Detection {
            let ctx = Context::with_ceiling(
                dir.path().join(relative_file),
                text.to_owned(),
                Some(dir.path()),
            );
            detect(&ctx)
        }

        #[test]
        fn file_in_vault_is_obsidian() {
            let dir = TempDir::new("detect-vault");
            fs::create_dir_all(dir.path().join("vault/.obsidian")).unwrap();
            let detection = detect_file(&dir, "vault/note.md", "plain text");
            assert_eq!(detection.dialect.id, "obsidian");
            assert_eq!(
                detection.reasons,
                [".obsidian folder found (Obsidian vault)"]
            );
        }

        #[test]
        fn file_in_repository_is_github_even_with_obsidian_syntax() {
            let dir = TempDir::new("detect-repo");
            fs::create_dir_all(dir.path().join("repo/.git")).unwrap();
            let detection = detect_file(&dir, "repo/doc.md", "[[link]] ==mark==");
            assert_eq!(detection.dialect.id, "github");
            assert_eq!(detection.reasons, ["Git repository found"]);
        }

        #[test]
        fn vault_from_obsidian_list_is_obsidian_even_in_a_repository() {
            let dir = TempDir::new("detect-known-vault");
            // A renamed settings folder, inside a Git repository.
            fs::create_dir_all(dir.path().join("repo/.git")).unwrap();
            fs::create_dir_all(dir.path().join("repo/Vault/my-config")).unwrap();
            let ctx = Context::with_ceiling(
                dir.path().join("repo/Vault/note.md"),
                "plain text".to_owned(),
                Some(dir.path()),
            )
            .with_known_vaults(&[dir.path().join("repo/Vault")]);
            let detection = detect(&ctx);
            assert_eq!(detection.dialect.id, "obsidian");
            assert_eq!(
                detection.reasons,
                ["vault listed in Obsidian (obsidian.json)"]
            );
        }

        #[test]
        fn both_vault_signals_are_reported() {
            let dir = TempDir::new("detect-both-signals");
            fs::create_dir_all(dir.path().join("Vault/.obsidian")).unwrap();
            let ctx = Context::with_ceiling(
                dir.path().join("Vault/n.md"),
                String::new(),
                Some(dir.path()),
            )
            .with_known_vaults(&[dir.path().join("Vault")]);
            assert_eq!(
                detect(&ctx).reasons,
                [
                    ".obsidian folder found (Obsidian vault)",
                    "vault listed in Obsidian (obsidian.json)"
                ]
            );
        }

        #[test]
        fn vault_inside_repository_is_obsidian() {
            let dir = TempDir::new("detect-vault-in-repo");
            fs::create_dir_all(dir.path().join("repo/.git")).unwrap();
            fs::create_dir_all(dir.path().join("repo/notes/.obsidian")).unwrap();
            assert_eq!(
                detect_file(&dir, "repo/notes/a.md", "").dialect.id,
                "obsidian"
            );
        }

        #[test]
        fn loose_file_with_obsidian_syntax_is_obsidian() {
            let dir = TempDir::new("detect-loose-obsidian");
            let detection = detect_file(&dir, "note.md", "See [[Other]] and ==this==.\n");
            assert_eq!(detection.dialect.id, "obsidian");
            assert_eq!(
                detection.reasons,
                ["wiki links found", "highlight (==text==) found"]
            );
        }

        #[test]
        fn obsidian_syntax_inside_code_does_not_count() {
            let dir = TempDir::new("detect-code");
            let text = "Use `[[link]]` like this:\n\n```\n[[Note]] ==x==\n```\n";
            let detection = detect_file(&dir, "guide.md", text);
            assert_eq!(detection.dialect.id, "github");
            assert_eq!(detection.reasons, [DEFAULT_REASON]);
        }

        #[test]
        fn loose_readme_is_github() {
            let dir = TempDir::new("detect-readme");
            let detection = detect_file(&dir, "README.md", "# Project\n");
            assert_eq!(detection.dialect.id, "github");
            assert_eq!(detection.reasons, ["typical repository file name"]);
        }
    }

    #[test]
    fn dialect_ids_are_unique_and_github_is_the_default() {
        assert_eq!(DIALECTS[0].id, "github");
        for (i, dialect) in DIALECTS.iter().enumerate() {
            assert!(
                DIALECTS[i + 1..].iter().all(|other| other.id != dialect.id),
                "duplicate id {}",
                dialect.id
            );
        }
    }
}
