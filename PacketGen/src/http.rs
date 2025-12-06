use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::stats::Stats;
use crate::http_util::build_http_request;

pub fn run_http(config: &Config, stats: Arc<Stats>) -> Vec<thread::JoinHandle<()>> {
    let mut handles = Vec::new();

    let threads = config.threads;
    let rps_per_thread = (config.pps / threads as u64).max(1);
    let dst = config.dst;
    let host = config.http_host.clone();
    let path = config.http_path.clone();
    let method = config.http_method;
    let body_size = config.http_body_size;

    for _ in 0..threads {
        let stats = stats.clone();
        let dst = dst.clone();
        let host = host.clone();
        let path = path.clone();

        let handle = thread::spawn(move || {
            let mut stream = match TcpStream::connect(dst) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("http connect error: {e}");
                    stats.inc_failed();
                    return;
                }
            };

            let _ = stream.set_nodelay(true);

            let req_bytes = build_http_request(method, &host, &path, body_size);

            let interval = Duration::from_secs_f64(1.0 / rps_per_thread as f64);
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

                if let Err(e) = stream.write_all(&req_bytes) {
                    eprintln!("http write error: {e}");
                    stats.inc_failed();
                    break;
                } else {
                    stats.inc_sent();
                }

                // 응답은 안 읽고 버림 (부하 생성 용도)
                next_send += interval;
            }
        });

        handles.push(handle);
    }

    handles
}
