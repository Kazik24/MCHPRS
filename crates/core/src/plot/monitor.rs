use mchprs_save_data::plot_data::Tps;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::warn;

const SAMPLES_PER_SECOND: usize = 2;
const MAX_SAMPLES: usize = 60 * SAMPLES_PER_SECOND;

#[derive(Default)]
struct AtomicTps {
    tps: AtomicU32,
    unlimited: AtomicBool,
}

impl AtomicTps {
    fn from_tps(tps: Tps) -> Self {
        match tps {
            Tps::Limited(tps) => AtomicTps {
                tps: AtomicU32::new(tps),
                unlimited: AtomicBool::new(false),
            },
            Tps::Unlimited => AtomicTps {
                tps: AtomicU32::new(0),
                unlimited: AtomicBool::new(true),
            },
        }
    }

    fn update(&self, tps: Tps) {
        match tps {
            Tps::Limited(tps) => {
                self.tps.store(tps, Ordering::Relaxed);
                self.unlimited.store(false, Ordering::Relaxed);
            }
            Tps::Unlimited => self.unlimited.store(true, Ordering::Relaxed),
        }
    }
}

struct MonitorData {
    tps: AtomicTps,
    ticks_passed: Arc<AtomicU64>,
    reset_timings: AtomicU32,
    too_slow: AtomicBool,
    ticking: AtomicBool,
    running: AtomicBool,
    timings_record: Mutex<VecDeque<u32>>,
}

#[derive(Debug)]
pub struct TimingsReport {
    pub two_s: f32,
    pub ten_s: f32,
    pub one_m: f32,
}

pub struct TimingsMonitor {
    data: Arc<MonitorData>,
    monitor_thread: Option<JoinHandle<()>>,
}

impl TimingsMonitor {
    pub fn new(tps: Tps) -> TimingsMonitor {
        let data = Arc::new(MonitorData {
            ticks_passed: Default::default(),
            reset_timings: Default::default(),
            running: AtomicBool::new(true),
            too_slow: Default::default(),
            ticking: Default::default(),
            timings_record: Default::default(),
            tps: AtomicTps::from_tps(tps),
        });
        let monitor_thread = Some(Self::run_thread(data.clone()));
        TimingsMonitor {
            data,
            monitor_thread,
        }
    }

    pub fn stop(&mut self) {
        self.data.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.monitor_thread.take() {
            if handle.join().is_err() {
                warn!("Failed to join monitor thread handle");
            }
        }
    }

    pub fn generate_report(&self) -> Option<TimingsReport> {
        let records = self.data.timings_record.lock().unwrap();
        if records.is_empty() {
            return None;
        }

        let average = |seconds: usize| {
            let samples = records.len().min(seconds * SAMPLES_PER_SECOND);
            let ticks: u64 = records
                .iter()
                .take(samples)
                .map(|&ticks| u64::from(ticks))
                .sum();
            ticks as f32 / samples as f32 * SAMPLES_PER_SECOND as f32
        };

        Some(TimingsReport {
            two_s: average(2),
            ten_s: average(10),
            one_m: average(60),
        })
    }

    pub fn set_tps(&self, new_tps: Tps) {
        self.data.tps.update(new_tps);
        self.data.too_slow.store(false, Ordering::Relaxed);
    }

    pub fn tick(&self) {
        self.data.ticks_passed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn is_running_behind(&self) -> bool {
        self.data.too_slow.load(Ordering::Relaxed)
    }

    pub fn set_ticking(&self, ticking: bool) {
        self.data.ticking.store(ticking, Ordering::Relaxed);
    }

    pub fn reset_timings(&self) {
        self.data.reset_timings.store(4, Ordering::Relaxed);
    }

    fn run_thread(data: Arc<MonitorData>) -> JoinHandle<()> {
        thread::spawn(move || {
            let mut last_tps = data.tps.tps.load(Ordering::Relaxed);
            let mut last_ticks_count = data.ticks_passed.load(Ordering::Relaxed);
            let mut was_ticking_before = data.ticking.load(Ordering::Relaxed);

            let mut behind_for = 0;
            loop {
                thread::sleep(Duration::from_secs(1) / SAMPLES_PER_SECOND as u32);
                if !data.running.load(Ordering::Relaxed) {
                    return;
                }

                let ticks_count = data.ticks_passed.load(Ordering::Relaxed);
                if ticks_count == 0 {
                    continue;
                }
                let ticks_passed = (ticks_count - last_ticks_count) as u32;
                last_ticks_count = ticks_count;

                let tps = data.tps.tps.load(Ordering::Relaxed);
                let ticking = data.ticking.load(Ordering::Relaxed);
                if !(ticking && was_ticking_before)
                    || tps != last_tps
                    || data.reset_timings.load(Ordering::Relaxed) > 0
                {
                    let _ = data.reset_timings.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |remaining| remaining.checked_sub(1),
                    );
                    was_ticking_before = ticking;
                    last_tps = tps;
                    continue;
                }

                // 5% threshold
                if data.tps.unlimited.load(Ordering::Relaxed)
                    || (ticks_passed as u64) < (tps as u64 / 2) * 95 / 100
                {
                    behind_for += 1;
                } else {
                    behind_for = 0;
                    data.too_slow.store(false, Ordering::Relaxed);
                }

                if behind_for >= 3 {
                    data.too_slow.store(true, Ordering::Relaxed);
                    // warn!(
                    //     "running behind by {} ticks",
                    //     ((tps / 2) * 95 / 100) - ticks_passed
                    // );
                }

                // Retain one minute at the 500 ms sample interval.
                let mut timings_record = data.timings_record.lock().unwrap();
                if timings_record.len() == MAX_SAMPLES {
                    timings_record.pop_back();
                }
                timings_record.push_front(ticks_passed);
            }
        })
    }
}

impl Drop for TimingsMonitor {
    fn drop(&mut self) {
        // Joining the thread in drop is a bad idea so we just let it detach
        self.data.running.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_monitor_does_not_underflow_reset_counter() {
        let mut monitor = TimingsMonitor::new(Tps::Limited(20));
        monitor.set_ticking(true);
        monitor.tick();
        thread::sleep(Duration::from_millis(600));
        monitor.stop();
        assert_eq!(monitor.data.reset_timings.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn high_tick_rate_reports_do_not_overflow() {
        let mut monitor = TimingsMonitor::new(Tps::Unlimited);
        monitor
            .data
            .timings_record
            .lock()
            .unwrap()
            .extend(std::iter::repeat_n(u32::MAX, MAX_SAMPLES));
        let report = monitor.generate_report().unwrap();
        assert!(report.one_m.is_finite());
        assert_eq!(report.one_m, u32::MAX as f32 * 2.0);
        monitor.stop();
    }
}
