//! Switch Explorer — Phase 0 spike (ADR-001 go/no-go gate).
//!
//! Throwaway code: it measures GPUI Kit against the Phase 0 criteria and is kept as a regression
//! harness for GPUI Kit upgrades. It is not part of the production workspace, so production rules
//! (i18n, theme tokens, `forbid(unsafe_code)` per crate) are deliberately relaxed here.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod bench;
mod data;
mod logger;
mod osd;
mod table;
mod win;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context as _, bail};
use gpui_kit::{application, assets, init};

/// Command-line options. Interactive by default; `--bench` runs one measurement and quits.
#[derive(Clone, Debug)]
pub struct Args {
    pub bench: Option<bench::Scenario>,
    pub seconds: f32,
    pub variant: data::Variant,
    pub source: data::Source,
    pub rows: usize,
    pub out_dir: PathBuf,
    pub trace: bool,
    pub own_drop_target: bool,
    pub opaque: bool,
    pub tag: String,
}

impl Args {
    fn parse() -> anyhow::Result<Self> {
        let mut args = Self {
            bench: None,
            seconds: 10.0,
            variant: data::Variant::DataTable,
            source: data::Source::Fixture,
            rows: 100_000,
            out_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out"),
            trace: false,
            own_drop_target: false,
            opaque: false,
            tag: String::from("adhoc"),
        };
        let mut source_set = false;
        let mut seconds_set = false;
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut value = |name: &str| it.next().with_context(|| format!("{name} needs a value"));
            match flag.as_str() {
                "--bench" => {
                    let v = value("--bench")?;
                    let scenario = bench::Scenario::parse(&v);
                    args.bench = Some(scenario.with_context(|| format!("unknown scenario {v}"))?);
                }
                "--seconds" => {
                    args.seconds = value("--seconds")?.parse()?;
                    seconds_set = true;
                }
                "--table" => args.variant = data::Variant::parse(&value("--table")?)?,
                "--data" => {
                    args.source = data::Source::parse(&value("--data")?)?;
                    source_set = true;
                }
                "--rows" => args.rows = value("--rows")?.parse()?,
                "--out" => args.out_dir = PathBuf::from(value("--out")?),
                "--tag" => args.tag = value("--tag")?,
                "--trace" => args.trace = true,
                "--own-drop" => args.own_drop_target = true,
                "--opaque" => args.opaque = true,
                other => bail!("unknown argument {other}"),
            }
        }
        if let Some(scenario) = args.bench {
            if scenario.is_scroll() && !source_set {
                args.source = data::Source::Synthetic;
            }
            if scenario == bench::Scenario::Idle && !seconds_set {
                args.seconds = 20.0;
            }
        }
        Ok(args)
    }
}

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    let args = Args::parse()?;
    logger::init(&args.out_dir, started)?;
    log::info!(
        target: "sx_spike",
        "start: {args:?} os_build={} engine={} profile={}",
        win::os_build(),
        app::ENGINE,
        app::PROFILE
    );
    application().with_assets(assets::Assets).run(move |cx| {
        init(cx);
        app::bind_keys(cx);
        if let Err(err) = app::open_main_window(args, started, cx) {
            log::error!(target: "sx_spike", "cannot open window: {err:#}");
            cx.quit();
        }
    });
    Ok(())
}
