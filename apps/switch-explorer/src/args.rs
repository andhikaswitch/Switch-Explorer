//! Command line (DEVELOPER-MODE.md §11). Options that belong to later phases are recognised and
//! rejected with a clear message instead of being silently ignored.

use std::path::PathBuf;

use anyhow::{Context as _, bail};

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub paths: Vec<PathBuf>,
    pub select: Option<PathBuf>,
    pub home: bool,
    pub safe_mode: bool,
    pub dev: bool,
    /// `--set id=value`, applied to the session only.
    pub sets: Vec<String>,
    /// `--lang en|id`: UI language override (default: the Windows display language).
    pub lang: Option<String>,
    /// `--bench-open PATH`: open, print startup and listing timings, quit.
    pub bench_open: Option<PathBuf>,
    /// `--appearance light|dark`: session override of the Windows app theme.
    pub light: Option<bool>,
}

const LATER: &[&str] = &[
    "--new-window",
    "--reset-theme",
    "--profile",
    "--resident",
    "--picker-host",
    "--upload-to-drive",
    "--register",
    "--unregister",
    "--picker-takeover",
];

impl Args {
    pub fn parse() -> anyhow::Result<Self> {
        Self::parse_from(
            std::env::args_os()
                .skip(1)
                .map(|a| a.to_string_lossy().into_owned()),
        )
    }

    pub fn parse_from(args: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let mut out = Self::default();
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            let mut value = |name: &str| it.next().with_context(|| format!("{name} needs a value"));
            match arg.as_str() {
                "--select" => out.select = Some(PathBuf::from(value("--select")?)),
                "--home" => out.home = true,
                "--safe-mode" => out.safe_mode = true,
                "--dev" => out.dev = true,
                "--set" => out.sets.push(value("--set")?),
                "--lang" => out.lang = Some(value("--lang")?),
                "--bench-open" => out.bench_open = Some(PathBuf::from(value("--bench-open")?)),
                "--appearance" => {
                    out.light = match value("--appearance")?.as_str() {
                        "light" => Some(true),
                        "dark" => Some(false),
                        "system" => None,
                        other => bail!("--appearance takes light, dark or system, not {other}"),
                    }
                }
                flag if LATER.contains(&flag) => {
                    bail!("{flag} is not available in this version yet")
                }
                flag if flag.starts_with("--") => bail!("unknown option {flag}"),
                path => out.paths.push(PathBuf::from(path)),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> anyhow::Result<Args> {
        Args::parse_from(args.iter().map(ToString::to_string))
    }

    #[test]
    fn paths_and_options() {
        let args = parse(&[
            r"C:\Users",
            "--set",
            "view.show_hidden=true",
            "--lang",
            "id",
        ])
        .unwrap();
        assert_eq!(args.paths, [PathBuf::from(r"C:\Users")]);
        assert_eq!(args.sets, ["view.show_hidden=true"]);
        assert_eq!(args.lang.as_deref(), Some("id"));
    }

    #[test]
    fn later_and_unknown_options_are_errors() {
        assert!(parse(&["--register", "win-e"]).is_err());
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["--set"]).is_err());
    }
}
