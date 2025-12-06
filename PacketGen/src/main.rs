mod config;
mod stats;
mod udp;
mod tcp;
mod http;
mod https;
mod http_util;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::config::{Config, Mode};
use crate::stats::{spawn_stats_thread, Stats};

fn main() {
    let config = Config::from_args();
    println!("Config: {:?}", config);

    let stats = Arc::new(Stats::new());
    let stats_thread = spawn_stats_thread(stats.clone());

    let handles = match config.mode {
        Mode::Udp => {
            println!("Running in UDP mode");
            udp::run_udp(&config, stats.clone())
        }
        Mode::Tcp => {
            println!("Running in TCP mode");
            tcp::run_tcp(&config, stats.clone())
        }
        Mode::Http => {
            println!("Running in HTTP mode");
            http::run_http(&config, stats.clone())
        }
        Mode::Https => {
            println!("Running in HTTPS mode");
            https::run_https(&config, stats.clone())
        }
    };

    thread::sleep(Duration::from_secs(config.duration_secs));
    stats.request_stop();

    for h in handles {
        let _ = h.join();
    }

    let _ = stats_thread.join();

    use std::sync::atomic::Ordering;
    println!(
        "Done. total_sent={}, total_failed={}",
        stats.sent.load(Ordering::Relaxed),
        stats.failed.load(Ordering::Relaxed)
    );
}
