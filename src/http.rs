use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use log::{info, error};

use crate::config::Config;
use crate::relay_info::RelayInfo;

pub async fn run_http_server(config: Config) -> Result<(), String> {
    let addr: SocketAddr = format!("0.0.0.0:{}", config.http_port)
        .parse()
        .map_err(|e| format!("Invalid address: {}", e))?;

    let listener = TcpListener::bind(addr).await
        .map_err(|e| format!("Failed to bind HTTP: {}", e))?;

    info!("HTTP server listening on {}", addr);

    loop {
        match listener.accept().await {
            Ok((mut stream, _)) => {
                let mut buf = [0u8; 2048];
                if let Ok(n) = stream.read(&mut buf).await {
                    let request = String::from_utf8_lossy(&buf[..n]);
                    if request.contains("Accept: application/nostr+json") || request.contains("Accept:application/nostr+json") {
                        let relay_info = RelayInfo::from_config(&config);
                        let json = relay_info.to_json();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\n\
                            Content-Type: application/nostr+json\r\n\
                            Access-Control-Allow-Origin: *\r\n\
                            Access-Control-Allow-Headers: Content-Type\r\n\
                            Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                            Content-Length: {}\r\n\r\n{}",
                            json.len(),
                            json
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                    } else if request.starts_with("GET / HTTP") {
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nHello";
                        let _ = stream.write_all(response.as_bytes()).await;
                    } else {
                        let response = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes()).await;
                    }
                }
            }
            Err(e) => {
                error!("HTTP accept error: {}", e);
            }
        }
    }
}