//! Serves the latest metrics snapshot on GET /metrics; every other request gets 404.

use std::time::Duration;

use log::{debug, info};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::metrics::Published;

const READ_TIMEOUT: Duration = Duration::from_secs(5);
// A request line and headers from a scraper fit well under this; the body of a GET is empty.
const MAX_REQUEST_BYTES: usize = 8 * 1024;

pub async fn run(listener: TcpListener, published: Published) {
    info!(
        "metrics on http://{}/metrics",
        listener
            .local_addr()
            .map_or_else(|e| e.to_string(), |a| a.to_string())
    );
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(answer(stream, published.clone()));
            }
            Err(e) => debug!("accept metrics connection: {e}"),
        }
    }
}

async fn answer(mut stream: TcpStream, published: Published) {
    let mut request = vec![0u8; MAX_REQUEST_BYTES];
    let read = match tokio::time::timeout(READ_TIMEOUT, stream.read(&mut request)).await {
        Ok(Ok(read)) => read,
        _ => return,
    };
    let response = respond(&request[..read], &published);
    if let Err(e) = stream.write_all(response.as_bytes()).await {
        debug!("write metrics response: {e}");
    }
}

fn respond(request: &[u8], published: &Published) -> String {
    let line = String::from_utf8_lossy(request);
    let mut parts = line.split_whitespace();
    let (status, body) = match (parts.next(), parts.next()) {
        (Some("GET"), Some("/metrics")) => (
            "200 OK",
            published
                .read()
                .map_or_else(|e| e.into_inner().clone(), |text| text.clone()),
        ),
        _ => ("404 Not Found", String::new()),
    };
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use super::*;

    #[test]
    fn metrics_path_returns_the_snapshot() {
        let published: Published = Arc::new(RwLock::new("x 1\n".into()));
        let response = respond(b"GET /metrics HTTP/1.1\r\nHost: a\r\n\r\n", &published);
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Length: 4\r\n"));
        assert!(response.ends_with("\r\n\r\nx 1\n"));
    }

    #[test]
    fn other_paths_are_not_found() {
        let published: Published = Arc::new(RwLock::new(String::new()));
        assert!(respond(b"GET / HTTP/1.1\r\n\r\n", &published).starts_with("HTTP/1.1 404"));
        assert!(respond(b"POST /metrics HTTP/1.1\r\n\r\n", &published).starts_with("HTTP/1.1 404"));
        assert!(respond(b"", &published).starts_with("HTTP/1.1 404"));
    }
}
