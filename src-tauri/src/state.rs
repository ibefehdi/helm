use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::bridge::{Bridge, BridgeError};
use crate::model::{Profile, Settings, save_json};
use crate::monitor::Monitor;
use crate::system;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Starting,
    Connecting,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    pub code: Option<String>,
    pub message: Option<String>,
    pub elevated: bool,
    pub service_state: Option<String>,
    pub bridge_version: Option<String>,
}

/// Duty cycles currently pinned through the engine's calibration override.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overrides {
    pub failsafe: bool,
    pub manual: BTreeMap<usize, u8>,
    pub calibrating: Option<usize>,
}

pub struct AppState {
    pub bridge_exe: PathBuf,
    pub data_dir: PathBuf,
    pub bridge: Mutex<Option<Arc<Bridge>>>,
    pub connect_lock: tokio::sync::Mutex<()>,
    pub status: Mutex<Status>,
    pub info: Mutex<Option<Value>>,
    pub settings: Mutex<Settings>,
    pub profiles: Mutex<Vec<Profile>>,
    pub monitor: Mutex<Monitor>,
    pub overrides: Mutex<Overrides>,
    pub cancel_calibration: AtomicBool,
    pub startup_profile_applied: AtomicBool,
    pub wake: Notify,
}

impl AppState {
    pub fn new(bridge_exe: PathBuf, data_dir: PathBuf, settings: Settings, profiles: Vec<Profile>) -> Self {
        Self {
            bridge_exe,
            data_dir,
            bridge: Mutex::new(None),
            connect_lock: tokio::sync::Mutex::new(()),
            status: Mutex::new(Status {
                phase: Phase::Starting,
                code: None,
                message: None,
                elevated: system::is_elevated(),
                service_state: None,
                bridge_version: None,
            }),
            info: Mutex::new(None),
            settings: Mutex::new(settings),
            profiles: Mutex::new(profiles),
            monitor: Mutex::new(Monitor::default()),
            overrides: Mutex::new(Overrides::default()),
            cancel_calibration: AtomicBool::new(false),
            startup_profile_applied: AtomicBool::new(false),
            wake: Notify::new(),
        }
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    pub fn profiles_path(&self) -> PathBuf {
        self.data_dir.join("profiles.json")
    }

    pub fn persist_settings(&self) -> Result<(), String> {
        let settings = self.settings.lock().unwrap().clone();
        save_json(&self.settings_path(), &settings).map_err(|e| e.to_string())
    }

    pub fn persist_profiles(&self) -> Result<(), String> {
        let profiles = self.profiles.lock().unwrap().clone();
        save_json(&self.profiles_path(), &profiles).map_err(|e| e.to_string())
    }
}

pub fn set_status(app: &AppHandle, update: impl FnOnce(&mut Status)) {
    let state = app.state::<AppState>();
    let snapshot = {
        let mut status = state.status.lock().unwrap();
        update(&mut status);
        status.clone()
    };
    let _ = app.emit("status", snapshot);
}

fn set_error(app: &AppHandle, err: &BridgeError) {
    set_status(app, |s| {
        s.phase = Phase::Error;
        s.code = Some(err.code.clone());
        s.message = Some(err.message.clone());
    });
}

/// Returns a live, connected bridge, spawning and handshaking a new one if needed.
pub async fn connection(app: &AppHandle) -> Result<Arc<Bridge>, BridgeError> {
    let state = app.state::<AppState>();
    if let Some(bridge) = live_bridge(&state) {
        return Ok(bridge);
    }

    let _guard = state.connect_lock.lock().await;
    if let Some(bridge) = live_bridge(&state) {
        return Ok(bridge);
    }

    if !system::is_elevated() {
        let err = BridgeError::new("not_admin", "Helm must run as administrator to reach the Gigabyte engine.");
        set_error(app, &err);
        return Err(err);
    }

    set_status(app, |s| {
        s.phase = Phase::Connecting;
        s.code = None;
        s.message = None;
    });

    let result = async {
        let bridge = Arc::new(Bridge::spawn(&state.bridge_exe)?);
        let hello = bridge.call("hello", json!({})).await?;
        set_status(app, |s| {
            s.service_state = hello["serviceState"].as_str().map(str::to_owned);
            s.bridge_version = hello["bridgeVersion"].as_str().map(str::to_owned);
        });
        let info = bridge.call("connect", json!({})).await?;
        Ok::<_, BridgeError>((bridge, info))
    }
    .await;

    match result {
        Ok((bridge, info)) => {
            *state.info.lock().unwrap() = Some(info.clone());
            *state.bridge.lock().unwrap() = Some(bridge.clone());
            set_status(app, |s| {
                s.phase = Phase::Ready;
                s.code = None;
                s.message = None;
            });
            let _ = app.emit("info", info);
            Ok(bridge)
        }
        Err(err) => {
            set_error(app, &err);
            Err(err)
        }
    }
}

fn live_bridge(state: &AppState) -> Option<Arc<Bridge>> {
    state.bridge.lock().unwrap().as_ref().filter(|b| b.is_alive()).cloned()
}

/// Records a failed call; connection-level faults drop the bridge so the next call respawns it.
pub fn report_fault(app: &AppHandle, err: &BridgeError) {
    if err.is_connection_fault() {
        let state = app.state::<AppState>();
        state.bridge.lock().unwrap().take();
        set_error(app, err);
    }
}

pub async fn call(app: &AppHandle, cmd: &str, args: Value) -> Result<Value, BridgeError> {
    let bridge = connection(app).await?;
    let result = bridge.call(cmd, args).await;
    if let Err(err) = &result {
        report_fault(app, err);
    }
    result
}

pub fn emit_overrides(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = state.overrides.lock().unwrap().clone();
    let _ = app.emit("overrides", snapshot);
}

pub fn request_calibration_cancel(state: &AppState) {
    state.cancel_calibration.store(true, Ordering::SeqCst);
}
