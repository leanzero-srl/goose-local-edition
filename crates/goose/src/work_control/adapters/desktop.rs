use leanzero_link::controller::ControllerError;
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Clone)]
pub struct DesktopBridge {
    socket: PathBuf,
    secret: String,
}

impl DesktopBridge {
    pub fn from_env() -> Option<Self> {
        let socket = std::env::var_os("GOOSE_CONTROLLER_DESKTOP_SOCKET")?;
        let secret = std::env::var("GOOSE_CONTROLLER_DESKTOP_SECRET").ok()?;
        if secret.is_empty() {
            return None;
        }
        Some(Self {
            socket: socket.into(),
            secret,
        })
    }

    #[cfg(unix)]
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, ControllerError> {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::AsyncReadExt;
        if !matches!(
            method,
            "benchmark.catalog" | "benchmark.command" | "benchmark.log"
        ) {
            return Err(ControllerError::new(
                axum::http::StatusCode::BAD_REQUEST,
                "invalid_request",
                "unknown desktop method",
                false,
            ));
        }
        let metadata = tokio::fs::metadata(&self.socket)
            .await
            .map_err(super::error)?;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(ControllerError::unavailable(
                "desktop socket permissions must be private",
            ));
        }
        let operation = async {
            let mut stream = tokio::net::UnixStream::connect(&self.socket)
                .await
                .map_err(super::error)?;
            let mut request =
                serde_json::to_vec(&json!({"secret":self.secret,"method":method,"params":params}))
                    .map_err(super::error)?;
            request.push(b'\n');
            stream.write_all(&request).await.map_err(super::error)?;
            let mut reader = BufReader::new(stream.take(1024 * 1024));
            let mut response = String::new();
            reader
                .read_line(&mut response)
                .await
                .map_err(super::error)?;
            if !response.ends_with('\n') {
                return Err(ControllerError::unavailable(
                    "desktop response is incomplete or too large",
                ));
            }
            let response: Value = serde_json::from_str(&response).map_err(super::error)?;
            if response["ok"].as_bool() == Some(true) {
                response
                    .get("result")
                    .cloned()
                    .ok_or_else(|| ControllerError::unavailable("desktop response has no result"))
            } else {
                Err(ControllerError::new(
                    axum::http::StatusCode::CONFLICT,
                    response["error"]["code"]
                        .as_str()
                        .unwrap_or("desktop_error"),
                    response["error"]["message"]
                        .as_str()
                        .unwrap_or("desktop command failed"),
                    false,
                ))
            }
        };
        tokio::time::timeout(Duration::from_secs(30), operation)
            .await
            .map_err(|_| {
                ControllerError::unavailable(
                    "desktop transport timed out; command outcome is unknown",
                )
            })?
    }

    #[cfg(not(unix))]
    pub async fn call(&self, _method: &str, _params: Value) -> Result<Value, ControllerError> {
        Err(ControllerError::unavailable(
            "desktop controller socket requires Unix",
        ))
    }
}
