use super::protocol::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex};

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>;

pub struct StdioTransport {
    #[allow(dead_code)]
    child: Child,
    stdin: Mutex<ChildStdin>,
    next_id: AtomicU64,
    pending: Pending,
}

impl StdioTransport {
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn `{command}` failed: {e}"))?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        {
            let pending = pending.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(line) else {
                        continue;
                    };
                    if let Some(id) = resp.id {
                        if let Some(tx) = pending.lock().await.remove(&id) {
                            let _ = tx.send(resp);
                        }
                    }
                }
            });
        }

        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        log::debug!("mcp stderr: {line}");
                    }
                    Ok(None) => break,
                    Err(e) => {
                        log::debug!("mcp stderr read error: {e}");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
            next_id: AtomicU64::new(1),
            pending,
        })
    }

    /// Default for ongoing `tools/call` traffic (container cold start is already paid at connect).
    pub fn default_call_timeout() -> Duration {
        Duration::from_secs(120)
    }

    pub async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        self.call_with_timeout(method, params, Self::default_call_timeout())
            .await
    }

    pub async fn call_with_timeout(
        &self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = JsonRpcRequest::new(id, method, params);
        let mut payload = serde_json::to_vec(&req).map_err(|e| format!("encode request: {e}"))?;
        payload.push(b'\n');

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        {
            let mut stdin = self.stdin.lock().await;
            if let Err(e) = stdin.write_all(&payload).await {
                self.pending.lock().await.remove(&id);
                return Err(format!("write stdin: {e}"));
            }
            if let Err(e) = stdin.flush().await {
                self.pending.lock().await.remove(&id);
                return Err(format!("flush: {e}"));
            }
        }

        let secs = timeout.as_secs().max(1);
        let resp = match tokio::time::timeout(timeout, rx).await {
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(format!("mcp call `{method}` timed out after {secs}s",));
            }
            Ok(rx_result) => match rx_result {
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    return Err("mcp response channel dropped".to_string());
                }
                Ok(resp) => resp,
            },
        };
        self.pending.lock().await.remove(&id);

        if let Some(err) = resp.error {
            return Err(format!("mcp error: {}", err.message));
        }
        Ok(resp.result.unwrap_or(Value::Null))
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let mut msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(p) = params {
            msg["params"] = p;
        }
        let mut payload = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        payload.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(&payload)
            .await
            .map_err(|e| format!("write stdin: {e}"))?;
        stdin.flush().await.map_err(|e| format!("flush: {e}"))?;
        Ok(())
    }
}
