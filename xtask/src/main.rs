//! Build automation (`cargo xtask <command>`).

use anyhow::bail;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => bail!("unknown xtask command `{other}`\n\n{USAGE}"),
    }
}

const USAGE: &str = "\
cargo xtask <command>

commands:
  fixtures --count <n>   create test folders under %TEMP%\\sx-fixtures
  cleanup-registry       remove every Switch Explorer registry integration (safe any time)
  shim                   build sx-picker-shim for x64 and x86";
