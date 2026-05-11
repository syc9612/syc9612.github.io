use crate::config::HttpMethod;

pub fn build_http_request(
    method: HttpMethod,
    host: &str,
    path: &str,
    body_size: usize,
) -> Vec<u8> {
    let method_str = match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
    };

    let body = if matches!(method, HttpMethod::Post) && body_size > 0 {
        Some(vec![b'x'; body_size])
    } else {
        None
    };

    let mut req = String::new();
    req.push_str(&format!("{method_str} {path} HTTP/1.1\r\n"));
    req.push_str(&format!("Host: {host}\r\n"));
    req.push_str("Connection: keep-alive\r\n");
    req.push_str("User-Agent: PacketGen/0.1\r\n");
    req.push_str("Accept: */*\r\n");

    if let Some(ref b) = body {
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
        req.push_str("Content-Type: application/octet-stream\r\n");
    }

    req.push_str("\r\n"); // 헤더 끝

    let mut bytes = req.into_bytes();

    if let Some(b) = body {
        bytes.extend_from_slice(&b);
    }

    bytes
}
