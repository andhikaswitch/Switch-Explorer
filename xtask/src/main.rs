//! Build automation (`cargo xtask <command>`).

mod fixtures;
mod registry;

use anyhow::{Context as _, bail};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|ix| args.get(ix + 1))
    };
    match args.first().map(String::as_str) {
        Some("fixtures") => {
            let count = value("--count")
                .map(|v| v.parse::<usize>())
                .transpose()
                .context("--count expects a number")?
                .unwrap_or(10_000);
            fixtures::run(count)
        }
        Some("cleanup-registry") => registry::cleanup(flag("--dry-run")),
        Some("shim") => shim(),
        None | Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => bail!("unknown xtask command `{other}`\n\n{USAGE}"),
    }
}

/// Builds the picker shim DLL. x86 joins when the shim gets real code (ADR-020).
fn shim() -> anyhow::Result<()> {
    let status = std::process::Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "-p",
            "sx-picker-shim",
            "--target",
            "x86_64-pc-windows-msvc",
        ])
        .status()
        .context("cannot run cargo")?;
    if !status.success() {
        bail!("building the x64 shim failed");
    }
    println!(
        "x64 shim built. The x86 build is postponed until Phase 6 (ADR-020): \
         add i686-pc-windows-msvc to rust-toolchain.toml then."
    );
    Ok(())
}

const USAGE: &str = "\
cargo xtask <command>

commands:
  fixtures [--count <n>]          create test folders under %TEMP%\\sx-fixtures (default 10000)
  cleanup-registry [--dry-run]    remove every Switch Explorer registry integration (safe any time)
  shim                            build sx-picker-shim (x64; x86 from Phase 6)";
