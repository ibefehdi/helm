//! Client for `helm-bridge.exe`, the .NET process that talks .NET Remoting to
//! Gigabyte's EasyTuneEngineService over the `GigabyteEngine` IPC pipe.

use std::collections::HashMap;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::oneshot;

const CALL_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize)]
pub struct BridgeError {
    pub code: String,
    pub message: String,
}

impl BridgeError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_owned(), message: message.into() }
    }

    /// Errors after which the bridge process should be discarded and respawned.
    pub fn is_connection_fault(&self) -> bool {
        matches!(self.code.as_str(), "bridge_exited" | "bridge_io" | "timeout" | "engine_unreachable")
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for BridgeError {}

type Reply = Result<Value, BridgeError>;
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Reply>>>>;

pub struct Bridge {
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    pending: Pending,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
}

impl Bridge {
    pub fn spawn(exe: &Path) -> Result<Self, BridgeError> {
        let mut command = Command::new(exe);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .map_err(|e| BridgeError::new("bridge_missing", format!("Could not start {}: {e}", exe.display())))?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stdin = child.stdin.take().expect("stdin is piped");

        let pending: Pending = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));

        let reader_pending = pending.clone();
        let reader_alive = alive.clone();
        std::thread::Builder::new()
            .name("helm-bridge-reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
                    let Some(id) = message.get("id").and_then(Value::as_u64) else { continue };
                    let Some(tx) = reader_pending.lock().unwrap().remove(&id) else { continue };
                    let reply = if message["ok"].as_bool() == Some(true) {
                        Ok(message["data"].clone())
                    } else {
                        Err(BridgeError::new(
                            message["code"].as_str().unwrap_or("error"),
                            message["error"].as_str().unwrap_or("Unknown bridge error"),
                        ))
                    };
                    let _ = tx.send(reply);
                }
                reader_alive.store(false, Ordering::SeqCst);
                for (_, tx) in reader_pending.lock().unwrap().drain() {
                    let _ = tx.send(Err(BridgeError::new("bridge_exited", "The bridge process exited.")));
                }
            })
            .map_err(|e| BridgeError::new("bridge_io", e.to_string()))?;

        Ok(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(Some(stdin)),
            pending,
            next_id: AtomicU64::new(1),
            alive,
        })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub async fn call(&self, cmd: &str, args: Value) -> Result<Value, BridgeError> {
        if !self.is_alive() {
            return Err(BridgeError::new("bridge_exited", "The bridge process exited."));
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let line = json!({ "id": id, "cmd": cmd, "args": args }).to_string();
        let written = {
            let mut guard = self.stdin.lock().unwrap();
            match guard.as_mut() {
                Some(stdin) => writeln!(stdin, "{line}").and_then(|_| stdin.flush()).map_err(|e| e.to_string()),
                None => Err("bridge stdin closed".to_owned()),
            }
        };
        if let Err(message) = written {
            self.pending.lock().unwrap().remove(&id);
            return Err(BridgeError::new("bridge_io", message));
        }

        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err(BridgeError::new("bridge_exited", "The bridge process exited.")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(BridgeError::new("timeout", format!("The engine did not answer '{cmd}' in time.")))
            }
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        // Closing stdin lets the bridge release any forced fan duty before it exits.
        self.stdin.lock().unwrap().take();
        let mut child = self.child.lock().unwrap();
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
    }
}
