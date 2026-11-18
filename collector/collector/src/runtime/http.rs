//! HTTP client for the collector, used to fetch metrics and send them to the API.

use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    // time::Duration,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail};

// TIMEOUT bounds the whole reqs, from connect to the last byte of the answer.
const TIMEOUT: Duration = Duration::from_secs(2);

pub struct Response {
    pub status: u16,
    pub body: String,
}

pub fn get(url: &str) -> anyhow::Result<String> {
    ok(request("GET", url, None)?, url)
}

pub fn merge_patch(url: &str, body: &str) -> anyhow::Result<String> {
    ok(
        request("PATCH", url, Some(("application/merge-patch+json", body)))?,
        url,
    )
}

fn ok(response: Response, url: &str) -> anyhow::Result<String> {
    if !(200..300).contains(&response.status) {
        bail!("{url}: HTTP {}: {}", response.status, response.body.trim());
    }
    Ok(response.body)
}

// request speaks HTTP/1.0, so the server closes the connection and never answers in chunks.
fn request(method: &str, url: &str, body: Option<(&str, &str)>) -> anyhow::Result<Response> {
    let deadline = Instant::now() + TIMEOUT;
    let (host, path) = split_url(url)?;
    let address = host
        .to_socket_addrs()
        .with_context(|| format!("resolve {host}"))?
        .next()
        .ok_or_else(|| anyhow!("no address for {host}"))?;
    let mut stream =
        TcpStream::connect_timeout(&address, TIMEOUT).with_context(|| format!("connect {host}"))?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut message = format!("{method} {path} HTTP/1.0\r\nHost: {host}\r\n");
    if let Some((content_type, content)) = body {
        message.push_str(&format!(
            "Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{content}",
            content.len()
        ));
    } else {
        message.push_str("\r\n");
    }
    stream.write_all(message.as_bytes())?;
    let raw = read_until(&mt stream, deadline).with_context(|| format!("read {url}"))?;
    // let mut raw = Vec::new();
    // stream
    //     .read_to_end(&mut raw)
    //     .with_context(|| format!("read {url}"))?;
    parse_response(&String::from_utf8_lossy(&raw))
}

fn read_until(stream: &mut TcpStream, deadline: Instant) -> anyhow::Result<Vec<u8>> {
    let mut raw = Vec::new();
    let mut buf = [0; 8192]; // 8 KiB is enough for a single HTTP header, and we don't care about the body size.
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            bail!("no complete answer within {TIMEOUT:?}");
        }
        stream.set_read_timeout(Some(left))?;
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(raw),
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn split_url(url: &str) -> anyhow::Result<(&str, &str)> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("{url}: only http:// is supported"))?;
    Ok(match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, "/"),
    })
}

fn parse_response(raw: &str) -> anyhow::Result<Response> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("response without header end"))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| anyhow!("unreadable status line"))?;
    Ok(Response {
        status,
        body: body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_host_and_path() {
        assert_eq!(
            split_url("http://10.0.0.1:9464/metrics").unwrap(),
            ("10.0.0.1:9464", "/metrics")
        );
        assert_eq!(
            split_url("http://127.0.0.1:8001").unwrap(),
            ("127.0.0.1:8001", "/")
        );
        assert!(split_url("https://kubernetes.default.svc/api").is_err());
    }

    #[test]
    fn reads_status_and_body() {
        let response = parse_response("HTTP/1.0 404 Not Found\r\nA: b\r\n\r\nmissing").unwrap();
        assert_eq!(response.status, 404);
        assert_eq!(response.body, "missing");
        assert!(parse_response("garbage").is_err());
    }
    #[test]
        fn a_server_that_trickles_bytes_ends_within_the_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            for _ in 0..20 {
                if stream.write_all(b"H").is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        let started = Instant::now();
        assert!(get(&format!("http://{address}/metrics")).is_err());
        assert!(started.elapsed() < TIMEOUT + Duration::from_millis(500));
    }
}
