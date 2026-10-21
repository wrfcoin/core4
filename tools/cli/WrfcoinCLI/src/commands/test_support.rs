use anyhow::{Result, anyhow};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub(crate) struct ExpectedRequest {
    pub method: &'static str,
    pub path: String,
    pub status: u16,
    pub body: String,
}

impl ExpectedRequest {
    pub(crate) fn new(
        method: &'static str,
        path: impl Into<String>,
        status: u16,
        body: impl Into<String>,
    ) -> Self {
        Self {
            method,
            path: path.into(),
            status,
            body: body.into(),
        }
    }
}

pub(crate) async fn spawn_mock_server(
    requests: Vec<ExpectedRequest>,
) -> Result<(String, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let queue = Arc::new(Mutex::new(VecDeque::from(requests)));
    let queue_for_task = queue.clone();

    let handle = tokio::spawn(async move {
        loop {
            let expected = {
                let mut guard = queue_for_task.lock().await;
                guard.pop_front()
            };

            let Some(expected) = expected else {
                break;
            };

            let (mut socket, _) = listener.accept().await?;
            let mut buf = vec![0_u8; 16 * 1024];
            let n = socket.read(&mut buf).await?;
            if n == 0 {
                return Err(anyhow!("mock server received empty request"));
            }

            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let mut lines = request.lines();
            let first_line = lines
                .next()
                .ok_or_else(|| anyhow!("mock server received malformed request"))?;
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap_or_default();
            let path = parts.next().unwrap_or_default();

            if method != expected.method {
                return Err(anyhow!(
                    "expected method {}, got {} for path {}",
                    expected.method,
                    method,
                    expected.path
                ));
            }
            if path != expected.path {
                return Err(anyhow!("expected path {}, got {}", expected.path, path));
            }

            let status_text = match expected.status {
                200 => "OK",
                201 => "Created",
                400 => "Bad Request",
                401 => "Unauthorized",
                404 => "Not Found",
                405 => "Method Not Allowed",
                500 => "Internal Server Error",
                _ => "OK",
            };

            let response = format!(
                "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                expected.status,
                status_text,
                expected.body.len(),
                expected.body
            );
            socket.write_all(response.as_bytes()).await?;
            socket.shutdown().await?;
        }

        Ok(())
    });

    Ok((format!("http://{}", addr), handle))
}
