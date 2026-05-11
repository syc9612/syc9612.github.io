use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

pub struct Stats {
    pub stop: AtomicBool,
    pub sent: AtomicU64,
    pub failed: AtomicU64,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            sent: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_sent(&self) {
        self.sent.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_failed(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn spawn_stats_thread(stats: Arc<Stats>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_sent = 0u64;
        let mut last_failed = 0u64;
        let mut last_time = Instant::now();

        while !stats.should_stop() {
            thread::sleep(Duration::from_secs(1));

            let now = Instant::now();
            let dt = now.duration_since(last_time).as_secs_f64();
            last_time = now;

            let sent = stats.sent.load(Ordering::Relaxed);
            let failed = stats.failed.load(Ordering::Relaxed);

            let sent_delta = sent - last_sent;
            let failed_delta = failed - last_failed;

            last_sent = sent;
            last_failed = failed;

            let pps = (sent_delta as f64) / dt.max(1e-6);

            println!(
                "[stats] sent={} (+{}), failed={} (+{}), pps={:.0}",
                sent, sent_delta, failed, failed_delta, pps
            );
        }

        println!(
            "[stats] stopped. total_sent={}, total_failed={}",
            stats.sent.load(Ordering::Relaxed),
            stats.failed.load(Ordering::Relaxed)
        );
    })
}
