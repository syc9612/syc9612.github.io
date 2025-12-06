use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use native_tls::TlsConnector;

use crate::config::Config;
use crate::stats::Stats;
use crate::http_util::build_http_request;

pub fn run_https(config: &Config, stats: Arc<Stats>) -> Vec<thread::JoinHandle<()>> {
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
            let tcp = match TcpStream::connect(dst) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("https tcp connect error: {e}");
                    stats.inc_failed();
                    return;
                }
            };

            let connector = match TlsConnector::new() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("tls connector error: {e}");
                    stats.inc_failed();
                    return;
                }
            };

            let mut stream = match connector.connect(&host, tcp) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("tls handshake error: {e}");
                    stats.inc_failed();
                    return;
                }
            };

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
                    eprintln!("https write error: {e}");
                    stats.inc_failed();
                    break;
                } else {
                    stats.inc_sent();
                }

                next_send += interval;
            }
        });

        handles.push(handle);
    }

    handles
}
