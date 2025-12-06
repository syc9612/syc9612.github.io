use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::stats::Stats;

pub fn run_tcp(config: &Config, stats: Arc<Stats>) -> Vec<thread::JoinHandle<()>> {
    let mut handles = Vec::new();

    let threads = config.threads;
    let pps_per_thread = (config.pps / threads as u64).max(1);
    let payload_size = config.payload_size;
    let dst = config.dst;

    for _ in 0..threads {
        let stats = stats.clone();
        let dst = dst.clone();

        let handle = thread::spawn(move || {
            let mut stream = match TcpStream::connect(dst) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("tcp connect error: {e}");
                    stats.inc_failed();
                    return;
                }
            };

            let _ = stream.set_nodelay(true);

            let payload = vec![0u8; payload_size];
            let interval = Duration::from_secs_f64(1.0 / pps_per_thread as f64);
            let mut next_send = Instant::now();

            while !stats.should_stop() {
                let now = Instant::now();
                if now < next_send {
                    let sleep_dur = next_send - now;
                    if sleep_dur > Duration::from_micros(10) {
                        thread::sleep(sleep_dur);
                    }
                } else {
                    next_send = now;
                }

                match stream.write_all(&payload) {
                    Ok(_) => stats.inc_sent(),
                    Err(e) => {
                        eprintln!("tcp write error: {e}");
                        stats.inc_failed();
                        break;
                    }
                }

                next_send += interval;
            }
        });

        handles.push(handle);
    }

    handles
}
