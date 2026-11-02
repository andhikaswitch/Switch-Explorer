//! Frame-time measurement on top of GPUI's profiler (gpui-kit feature `profiler`).
//!
//! Scroll scenarios move the list every frame, time-based (a late frame jumps further, like real
//! scrolling), and record present intervals and draw durations after a warm-up. Results go to
//! `out/` as one CSV per run plus a line in `out/summary.tsv`.

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::profiler::{self, FrameEvent, FrameTimingCollector};

pub const WARMUP: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Idle,
    Startup,
    ScrollSlow,
    ScrollFast,
    ScrollJump,
    ScrollPixel,
}

impl Scenario {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "idle" => Self::Idle,
            "startup" => Self::Startup,
            "scroll-slow" => Self::ScrollSlow,
            "scroll-fast" => Self::ScrollFast,
            "scroll-jump" => Self::ScrollJump,
            "scroll-pixel" => Self::ScrollPixel,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Startup => "startup",
            Self::ScrollSlow => "scroll-slow",
            Self::ScrollFast => "scroll-fast",
            Self::ScrollJump => "scroll-jump",
            Self::ScrollPixel => "scroll-pixel",
        }
    }

    pub fn is_scroll(self) -> bool {
        matches!(
            self,
            Self::ScrollSlow | Self::ScrollFast | Self::ScrollJump | Self::ScrollPixel
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ScrollTarget {
    Row(usize),
    Pixel(f32),
}

/// Presented frames seen by GPUI's profiler, kept for latency probes.
pub struct FrameLog {
    collector: FrameTimingCollector,
    presents: VecDeque<Instant>,
}

impl FrameLog {
    pub fn new() -> Self {
        profiler::set_trace_enabled(true);
        Self {
            collector: FrameTimingCollector::new(),
            presents: VecDeque::new(),
        }
    }

    pub fn poll(&mut self) -> Vec<FrameEvent> {
        let events = self.collector.collect_unseen();
        for event in &events {
            if let FrameEvent::Present(present) = event {
                self.presents.push_back(present.present_end);
            }
        }
        while self.presents.len() > 4096 {
            self.presents.pop_front();
        }
        events
    }

    pub fn first_present_after(&self, t: Instant) -> Option<Instant> {
        self.presents.iter().copied().find(|p| *p >= t)
    }
}

/// Context written next to every result line.
pub struct RunInfo {
    pub tag: String,
    pub variant: &'static str,
    pub rows: usize,
    pub engine: &'static str,
    pub profile: &'static str,
    pub adapter: String,
    pub backdrop: &'static str,
    pub private_ws_mb: f64,
    pub os_build: u32,
}

pub struct Bench {
    pub scenario: Scenario,
    duration: Duration,
    start: Option<Instant>,
    log: FrameLog,
    presents: Vec<Instant>,
    draws: Vec<Duration>,
    refresh: Option<Duration>,
    rng: u64,
}

impl Bench {
    pub fn new(scenario: Scenario, seconds: f32) -> Self {
        Self {
            scenario,
            duration: Duration::from_secs_f32(seconds),
            start: None,
            log: FrameLog::new(),
            presents: Vec::new(),
            draws: Vec::new(),
            refresh: None,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn started(&self) -> bool {
        self.start.is_some()
    }

    pub fn begin(&mut self, now: Instant) {
        self.start = Some(now);
    }

    pub fn frame_log(&mut self) -> &mut FrameLog {
        &mut self.log
    }

    /// Collects profiler events; only frames inside the measured window count.
    pub fn collect(&mut self) {
        let events = self.log.poll();
        let Some(start) = self.start else {
            return;
        };
        let from = start + WARMUP;
        let until = from + self.duration;
        for event in events {
            match event {
                FrameEvent::Draw(frame)
                    if frame.draw_start >= from && frame.draw_start <= until =>
                {
                    self.draws.push(frame.draw_duration());
                    if frame.refresh_interval.is_some() {
                        self.refresh = frame.refresh_interval;
                    }
                }
                FrameEvent::Present(present)
                    if present.present_end >= from && present.present_end <= until =>
                {
                    self.presents.push(present.present_end);
                }
                _ => {}
            }
        }
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        self.start
            .is_some_and(|s| now >= s + WARMUP + self.duration + Duration::from_millis(250))
    }

    /// Where the list should be "now" for this scenario.
    pub fn target(&mut self, now: Instant, rows: usize) -> Option<ScrollTarget> {
        let start = self.start?;
        if rows == 0 {
            return None;
        }
        let t = now.duration_since(start).as_secs_f64();
        Some(match self.scenario {
            Scenario::ScrollSlow => ScrollTarget::Row((t * 60.0) as usize % rows),
            Scenario::ScrollFast => ScrollTarget::Row((t * 600.0) as usize % rows),
            Scenario::ScrollJump => {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 7;
                self.rng ^= self.rng << 17;
                ScrollTarget::Row((self.rng % rows as u64) as usize)
            }
            Scenario::ScrollPixel => {
                let span = (rows.saturating_sub(40) as f64 * 28.0).max(1.0);
                ScrollTarget::Pixel(((t * 2400.0) % span) as f32)
            }
            Scenario::Idle | Scenario::Startup => return None,
        })
    }

    /// Writes the per-frame CSV and appends one summary line; returns that line.
    pub fn write(&self, info: &RunInfo, out_dir: &Path) -> std::io::Result<String> {
        let stamp = unix_stamp();
        let mut intervals: Vec<f64> = self
            .presents
            .windows(2)
            .map(|w| ms(w[1].duration_since(w[0])))
            .collect();
        let mut draws: Vec<f64> = self.draws.iter().map(|d| ms(*d)).collect();

        let csv_path = out_dir.join(format!(
            "{}-{}-{}-{stamp}.csv",
            info.tag,
            self.scenario.name(),
            info.variant
        ));
        let mut csv = String::from("index,present_interval_ms,draw_ms\n");
        for i in 0..intervals.len().max(draws.len()) {
            let cell = |v: Option<&f64>| v.map(|v| format!("{v:.3}")).unwrap_or_default();
            csv.push_str(&format!(
                "{i},{},{}\n",
                cell(intervals.get(i)),
                cell(draws.get(i))
            ));
        }
        std::fs::write(csv_path, csv)?;

        intervals.sort_by(f64::total_cmp);
        draws.sort_by(f64::total_cmp);
        let over = |limit: f64| intervals.iter().filter(|v| **v > limit).count();
        let fps = self.presents.len() as f64 / self.duration.as_secs_f64();
        let refresh_hz = self.refresh.map_or(f64::NAN, |r| 1.0 / r.as_secs_f64());
        let line = format!(
            "{stamp}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{refresh_hz:.0}\t{}\t{fps:.1}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{}\t{}\t{:.1}\n",
            info.tag,
            self.scenario.name(),
            info.variant,
            info.rows,
            info.engine,
            info.profile,
            info.backdrop,
            info.adapter,
            info.os_build,
            self.presents.len(),
            percentile(&intervals, 0.50),
            percentile(&intervals, 0.95),
            percentile(&intervals, 0.99),
            intervals.last().copied().unwrap_or(f64::NAN),
            percentile(&draws, 0.50),
            percentile(&draws, 0.95),
            percentile(&draws, 0.99),
            draws.last().copied().unwrap_or(f64::NAN),
            over(16.7),
            over(25.0),
            info.private_ws_mb,
        );
        append_line(
            &out_dir.join("summary.tsv"),
            "stamp\ttag\tscenario\tvariant\trows\tengine\tprofile\tbackdrop\tadapter\tos_build\trefresh_hz\tframes\tfps\tint_p50\tint_p95\tint_p99\tint_max\tdraw_p50\tdraw_p95\tdraw_p99\tdraw_max\tint_gt_16_7\tint_gt_25\tprivate_ws_mb\n",
            &line,
        )?;
        Ok(line)
    }
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

pub fn unix_stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Appends `line` to a TSV file, writing `header` first when the file is new.
pub fn append_line(path: &Path, header: &str, line: &str) -> std::io::Result<()> {
    let is_new = !path.exists();
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if is_new {
        file.write_all(header.as_bytes())?;
    }
    file.write_all(line.as_bytes())
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

#[cfg(test)]
mod tests {
    use super::percentile;

    #[test]
    fn percentiles() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 0.50), 50.0);
        assert_eq!(percentile(&v, 0.99), 99.0);
        assert!(percentile(&[], 0.5).is_nan());
    }
}
