//! A transfer rate worth showing a "time left" for (walkthrough finding 2 of
//! `docs/plans/m5-walkthrough-findings.md`): a plan starts with its small
//! files (pictures, CFG), whose per-file cost on FAT32 makes the first
//! bytes-per-second meaningless, so the rate waits for enough time and
//! bytes, follows a sliding window rather than the average since the start,
//! and is smoothed between readings.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// No rate before this much time has passed...
pub(crate) const WARM_UP: Duration = Duration::from_secs(3);
/// ...and this many bytes went through.
pub(crate) const WARM_UP_BYTES: u64 = 4 << 20;
/// The rate is measured over the readings of the last window.
const WINDOW: Duration = Duration::from_secs(20);
/// How much a new reading moves the shown rate.
const SMOOTHING: f64 = 0.3;

pub(crate) struct RateMeter {
    started: Instant,
    /// `(when, bytes done)`, oldest first; the first one is the window's start.
    samples: VecDeque<(Instant, u64)>,
    shown: Option<f64>,
}

impl RateMeter {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            started: now,
            samples: VecDeque::from([(now, 0)]),
            shown: None,
        }
    }

    /// Records `done` bytes at `now` and returns the rate to show, in bytes
    /// per second, once there is one.
    pub(crate) fn record(&mut self, now: Instant, done: u64) -> Option<u64> {
        self.samples.push_back((now, done));
        while self.samples.len() > 2
            && self
                .samples
                .get(1)
                .is_some_and(|(at, _)| now.duration_since(*at) >= WINDOW)
        {
            self.samples.pop_front();
        }
        if now.duration_since(self.started) < WARM_UP || done < WARM_UP_BYTES {
            return None;
        }
        let (from, from_done) = *self.samples.front()?;
        let secs = now.duration_since(from).as_secs_f64();
        if secs <= 0.0 {
            return self.shown.map(|r| r as u64);
        }
        let current = done.saturating_sub(from_done) as f64 / secs;
        let shown = match self.shown {
            None => current,
            Some(previous) => previous * (1.0 - SMOOTHING) + current * SMOOTHING,
        };
        self.shown = Some(shown);
        Some(shown as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1 << 20;

    fn at(start: Instant, secs: f64) -> Instant {
        start + Duration::from_secs_f64(secs)
    }

    #[test]
    fn no_rate_before_the_warm_up() {
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        // Fast, but too early to tell.
        assert_eq!(meter.record(at(start, 1.0), 10 * MIB), None);
        // Long enough, but too little went through: small files so far.
        assert_eq!(meter.record(at(start, 3.0), MIB), None);
        // Both: a rate.
        let rate = meter.record(at(start, 4.0), 8 * MIB).unwrap();
        assert!(
            (rate as f64 - 2.0 * MIB as f64).abs() < 0.05 * MIB as f64,
            "{rate}"
        );
    }

    #[test]
    fn follows_the_window_not_the_average_since_the_start() {
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        // Ten seconds of small files at 1 MiB/s, then a big image at 20 MiB/s.
        let mut done = 0;
        let mut last = None;
        for second in 1..=10 {
            done += MIB;
            last = meter.record(at(start, f64::from(second)), done);
        }
        assert!(last.is_some_and(|r| r <= MIB + MIB / 10), "{last:?}");
        for second in 11..=50 {
            done += 20 * MIB;
            last = meter.record(at(start, f64::from(second)), done);
        }
        // The average since the start would say about 16 MiB/s; the window says 20.
        let rate = last.unwrap();
        assert!(rate > 19 * MIB && rate <= 20 * MIB + MIB / 10, "{rate}");
    }

    #[test]
    fn a_stall_moves_the_rate_a_little_not_to_zero() {
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        let mut done = 0;
        let mut rate = 0;
        for second in 1..=30 {
            done += 10 * MIB;
            rate = meter
                .record(at(start, f64::from(second)), done)
                .unwrap_or(0);
        }
        // Nothing for two seconds: the shown rate eases, it does not collapse.
        let stalled = meter.record(at(start, 32.0), done).unwrap();
        assert!(
            stalled < rate && stalled > rate * 8 / 10,
            "{rate} -> {stalled}"
        );
    }
}
