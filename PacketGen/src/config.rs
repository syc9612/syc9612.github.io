use std::env;
use std::net::{IpAddr, SocketAddr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Udp,
    Tcp,
    Http,
    Https,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub dst: SocketAddr,
    pub pps: u64,
    pub payload_size: usize,
    pub threads: usize,
    pub duration_secs: u64,

    pub http_host: String,
    pub http_path: String,
    pub http_method: HttpMethod,
    pub http_body_size: usize,
}

impl Config {
    pub fn from_args() -> Self {
        let mut mode = Mode::Udp;
        let mut dst_ip: Option<String> = None;
        let mut dst_port: u16 = 5000;
        let mut pps: u64 = 1_000;
        let mut payload_size: usize = 512;
        let mut threads: usize = 1;
        let mut duration_secs: u64 = 10;

        let mut http_host = String::from("localhost");
        let mut http_path = String::from("/");
        let mut http_method = HttpMethod::Get;
        let mut http_body_size: usize = 0;

        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--mode" => {
                    if let Some(v) = args.next() {
                        mode = match v.as_str() {
                            "udp" => Mode::Udp,
                            "tcp" => Mode::Tcp,
                            "http" => Mode::Http,
                            "https" => Mode::Https,
                            other => panic!("unknown mode: {other} (use udp|tcp|http|https)"),
                        };
                    }
                }
                "--dst-ip" => {
                    if let Some(v) = args.next() {
                        dst_ip = Some(v);
                    }
                }
                "--dst-port" => {
                    if let Some(v) = args.next() {
                        dst_port = v.parse().expect("invalid dst-port");
                    }
                }
                "--pps" => {
                    if let Some(v) = args.next() {
                        pps = v.parse().expect("invalid pps");
                    }
                }
                "--payload-size" => {
                    if let Some(v) = args.next() {
                        payload_size = v.parse().expect("invalid payload-size");
                    }
                }
                "--threads" => {
                    if let Some(v) = args.next() {
                        threads = v.parse().expect("invalid threads");
                    }
                }
                "--duration" => {
                    if let Some(v) = args.next() {
                        duration_secs = v.parse().expect("invalid duration");
                    }
                }
                "--http-host" => {
                    if let Some(v) = args.next() {
                        http_host = v;
                    }
                }
                "--http-path" => {
                    if let Some(v) = args.next() {
                        http_path = v;
                    }
                }
                "--http-method" => {
                    if let Some(v) = args.next() {
                        http_method = match v.to_lowercase().as_str() {
                            "get" => HttpMethod::Get,
                            "post" => HttpMethod::Post,
                            other => panic!("unknown http method: {other} (use get|post)"),
                        };
                    }
                }
                "--http-body-size" => {
                    if let Some(v) = args.next() {
                        http_body_size = v.parse().expect("invalid http-body-size");
                    }
                }
                other => {
                    eprintln!("Unknown arg: {other}");
                }
            }
        }

        let dst_ip = dst_ip.unwrap_or_else(|| "127.0.0.1".to_string());
        let ip: IpAddr = dst_ip.parse().expect("invalid dst-ip");
        let dst = SocketAddr::new(ip, dst_port);

        Config {
            mode,
            dst,
            pps,
            payload_size,
            threads: threads.max(1),
            duration_secs,
            http_host,
            http_path,
            http_method,
            http_body_size,
        }
    }
}
