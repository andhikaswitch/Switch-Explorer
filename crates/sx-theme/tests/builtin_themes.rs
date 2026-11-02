//! CI gate (THEMING.md §11): every built-in theme resolves without warnings and passes every
//! contrast pair in every variant, in translucent and solid material.

use sx_theme::{Mode, ResolveOptions, ThemeFile, builtin, resolve, validate};

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
