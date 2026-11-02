//! CI gate (THEMING.md §11): every built-in theme resolves without warnings and passes every
//! contrast pair in every variant, in translucent and solid material.

use sx_theme::{Mode, Oklch, ResolveOptions, ThemeFile, builtin, resolve, validate};

/// Accent seeds a user may pick (THEMING.md §9): 12 hues × dark/mid/light, vivid and muted.
fn sweep_seeds() -> Vec<sx_theme::Rgba> {
    let mut seeds = Vec::new();
    for hue in (0..360).step_by(30) {
        for (l, c) in [(0.50, 0.12), (0.68, 0.18), (0.82, 0.10)] {
            seeds.push(
                Oklch {
                    l,
                    c,
                    h: hue as f32,
                }
                .to_rgba(1.0),
            );
        }
    }
    seeds
}

#[test]
fn custom_accents_keep_builtin_themes_accessible() {
    let mut failures = Vec::new();
    for (id, source) in builtin::BUILTIN {
        let file = ThemeFile::parse(source).unwrap();
        for seed in sweep_seeds() {
            for mode in variants(&file) {
                let options = ResolveOptions {
                    accent: Some(seed),
                    ..ResolveOptions::default()
                };
                let theme = resolve(&file, mode, &options).unwrap();
                for issue in validate::check(&theme) {
                    failures.push(format!(
                        "{id} {mode:?} accent {}: {} on {} is {:.2}:1 (needs {}:1)",
                        seed.to_hex(),
                        issue.fg.id(),
                        issue.bg.id(),
                        issue.ratio,
                        issue.min
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn variants(file: &ThemeFile) -> Vec<Mode> {
    file.meta
        .variants
        .iter()
        .map(|v| match v.as_str() {
            "light" => Mode::Light,
            "dark" => Mode::Dark,
            other => panic!("{}: unknown variant {other}", file.meta.id),
        })
        .collect()
}

#[test]
fn builtin_themes_resolve_cleanly_and_pass_contrast() {
    let mut failures = Vec::new();
    for (id, source) in builtin::BUILTIN {
        let file = ThemeFile::parse(source).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(file.meta.id, *id);
        for mode in variants(&file) {
            for solid in [false, true] {
                let options = ResolveOptions {
                    solid,
                    ..ResolveOptions::default()
                };
                let theme = resolve(&file, mode, &options).unwrap();
                for w in &theme.warnings {
                    failures.push(format!("{id} {mode:?} solid={solid}: {w}"));
                }
                for issue in validate::check(&theme) {
                    failures.push(format!(
                        "{id} {mode:?} solid={solid}: {} on {} is {:.2}:1 (needs {}:1)",
                        issue.fg.id(),
                        issue.bg.id(),
                        issue.ratio,
                        issue.min
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
