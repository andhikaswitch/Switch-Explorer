//! Minimal `log` backend: timestamps relative to process start, written to a per-process file
//! (and stderr in debug builds, which have a console).

use std::fs::File;
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

struct SpikeLogger {
    start: Instant,
    file: Mutex<File>,
}

impl log::Log for SpikeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{:>10.3}ms {:<5} {}: {}\n",
            self.start.elapsed().as_secs_f64() * 1000.0,
            record.level(),
            record.target(),
            record.args()
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

pub fn init(out_dir: &Path, start: Instant) -> anyhow::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let file = File::create(out_dir.join(format!("spike-{}.log", std::process::id())))?;
    log::set_boxed_logger(Box::new(SpikeLogger {
        start,
        file: Mutex::new(file),
    }))?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}
