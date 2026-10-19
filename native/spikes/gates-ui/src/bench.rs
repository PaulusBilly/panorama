use crate::win32::{Memory, memory};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

pub type Reports = Arc<Mutex<Vec<String>>>;

pub struct Samples {
    pub start: Instant,
    last: Instant,
    log: Instant,
    pub intervals: Vec<f64>,
    pub start_memory: Memory,
    peak: Memory,
    log_frames: usize,
    log_worst: f64,
    reports: Option<Reports>,
}

impl Samples {
    pub fn new(now: Instant) -> Result<Self, String> {
        let mem = memory()?;
        Ok(Self {
            start: now,
            last: now,
            log: now,
            intervals: Vec::new(),
            start_memory: mem,
            peak: mem,
            log_frames: 0,
            log_worst: 0.0,
            reports: None,
        })
    }
    pub fn deferred(now: Instant, reports: Reports) -> Result<Self, String> {
        let mut samples = Self::new(now)?;
        samples.reports = Some(reports);
        Ok(samples)
    }
    fn report(&self, line: String) {
        if let Some(reports) = &self.reports {
            reports.lock().unwrap().push(line);
        } else {
            println!("{line}");
        }
    }
    pub fn frame(&mut self, now: Instant) {
        let ms = now.duration_since(self.last).as_secs_f64() * 1000.0;
        self.intervals.push(ms);
        self.log_worst = self.log_worst.max(ms);
        self.last = now;
    }
    pub fn log(
        &mut self,
        now: Instant,
        pass: usize,
        cache_bytes: usize,
        cache_entries: usize,
        ready: usize,
        visible: usize,
    ) -> Result<(), String> {
        if now.duration_since(self.log).as_secs_f64() < 1.0 {
            return Ok(());
        }
        let mem = memory()?;
        self.peak.working = self.peak.working.max(mem.working);
        self.peak.private = self.peak.private.max(mem.private);
        self.report(format!(
            "scroll pass={pass} second={:.3} frames={} interval_s={:.3} worst_ms={:.3} working_mib={:.3} private_mib={:.3} cache_mib={:.3} cache_entries={cache_entries} loaded_visible={ready}/{visible}",
            now.duration_since(self.start).as_secs_f64(),
            self.intervals.len() - self.log_frames,
            now.duration_since(self.log).as_secs_f64(),
            self.log_worst,
            mib(mem.working),
            mib(mem.private),
            mib(cache_bytes)
        ));
        self.log = now;
        self.log_frames = self.intervals.len();
        self.log_worst = 0.0;
        Ok(())
    }
    pub fn observe_memory(&mut self) -> Result<(), String> {
        let mem = memory()?;
        self.peak.working = self.peak.working.max(mem.working);
        self.peak.private = self.peak.private.max(mem.private);
        Ok(())
    }
    pub fn finish(&mut self, label: &str, now: Instant, refresh: u32) -> Result<Memory, String> {
        self.observe_memory()?;
        let end = memory()?;
        let elapsed = now.duration_since(self.start).as_secs_f64();
        let mut sorted = self.intervals.clone();
        sorted.sort_by(f64::total_cmp);
        let slow_count = sorted.len().div_ceil(100).max(1);
        let low_ms = sorted.iter().rev().take(slow_count).sum::<f64>() / slow_count as f64;
        let fps = self.intervals.len() as f64 / elapsed;
        let low = 1000.0 / low_ms;
        let worst = sorted.last().copied().unwrap_or(0.0);
        self.report(format!(
            "SUMMARY {label} frames={} elapsed_s={elapsed:.3} avg_fps={fps:.3} low_1pct_fps={low:.3} worst_ms={worst:.3} refresh_hz={refresh} fps_thresholds={} working_start_peak_end_mib={:.3}/{:.3}/{:.3} private_start_peak_end_mib={:.3}/{:.3}/{:.3}",
            self.intervals.len(),
            if fps >= f64::from(refresh) * 0.95 && low >= f64::from(refresh) * 0.5 {
                "PASS"
            } else {
                "FAIL"
            },
            mib(self.start_memory.working),
            mib(self.peak.working),
            mib(end.working),
            mib(self.start_memory.private),
            mib(self.peak.private),
            mib(end.private)
        ));
        Ok(end)
    }
    pub fn motion(&self, label: &str, run: usize, now: Instant) {
        println!(
            "motion {label} run={run} frames={} elapsed_ms={:.3} worst_ms={:.3}",
            self.intervals.len(),
            now.duration_since(self.start).as_secs_f64() * 1000.0,
            self.intervals.iter().copied().fold(0.0, f64::max)
        );
    }
}

pub fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
