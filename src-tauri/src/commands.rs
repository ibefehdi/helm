use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::model::{FanCurve, LightingPreset, LightingZone, Profile, Settings, import_gcc_profiles};
use crate::monitor::Sample;
use crate::state::{self, AppState, Overrides, Status};
use crate::{gpu, logitech, storage, system, updates};

type CmdResult<T> = Result<T, String>;

const GCC_FAN_PROFILE_DIR: &str = r"C:\Program Files\GIGABYTE\Control Center\Fan";
/// Calibration aborts above this CPU temperature because it can hold fans at low duty.
const CALIBRATION_CPU_LIMIT: f32 = 85.0;

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn fan_count(state: &AppState) -> usize {
    state.info.lock().unwrap().as_ref().and_then(|i| i["fanCount"].as_u64()).unwrap_or(0) as usize
}

// ---- connection -------------------------------------------------------------

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Status {
    state.status.lock().unwrap().clone()
}

#[tauri::command]
pub async fn get_info(app: AppHandle) -> CmdResult<Value> {
    state::connection(&app).await.map_err(err)?;
    let state = app.state::<AppState>();
    let info = state.info.lock().unwrap().clone();
    info.ok_or_else(|| "Not connected".into())
}

#[tauri::command]
pub async fn reconnect(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    state.bridge.lock().unwrap().take();
    state::connection(&app).await.map_err(err)?;
    state.wake.notify_one();
    Ok(())
}

#[tauri::command]
pub fn relaunch_as_admin(app: AppHandle) -> CmdResult<()> {
    if system::relaunch_elevated("") {
        app.exit(0);
        Ok(())
    } else {
        Err("Elevation was cancelled.".into())
    }
}

// ---- fans -------------------------------------------------------------------

#[tauri::command]
pub async fn get_fans(app: AppHandle) -> CmdResult<Vec<FanCurve>> {
    let raw = state::call(&app, "fan.getAll", json!({})).await.map_err(err)?;
    serde_json::from_value(raw).map_err(err)
}

async fn write_curve(app: &AppHandle, curve: &FanCurve) -> CmdResult<FanCurve> {
    let raw = state::call(app, "fan.set", serde_json::to_value(curve).map_err(err)?).await.map_err(err)?;
    serde_json::from_value(raw).map_err(err)
}

fn mark_custom(app: &AppHandle) {
    let state = app.state::<AppState>();
    let changed = state.settings.lock().unwrap().active_profile.take().is_some();
    if changed {
        let _ = state.persist_settings();
        emit_settings(app);
    }
}

#[tauri::command]
pub async fn set_fan(app: AppHandle, curve: FanCurve) -> CmdResult<FanCurve> {
    let written = write_curve(&app, &curve).await?;
    mark_custom(&app);
    Ok(written)
}

#[tauri::command]
pub async fn set_fans(app: AppHandle, curves: Vec<FanCurve>) -> CmdResult<Vec<FanCurve>> {
    let mut written = Vec::with_capacity(curves.len());
    for curve in &curves {
        written.push(write_curve(&app, curve).await?);
    }
    mark_custom(&app);
    Ok(written)
}

#[tauri::command]
pub async fn force_fan(app: AppHandle, index: usize, percent: u8) -> CmdResult<()> {
    let state = app.state::<AppState>();
    {
        let overrides = state.overrides.lock().unwrap();
        if overrides.failsafe {
            return Err("The thermal failsafe is holding all fans at 100%.".into());
        }
        if overrides.calibrating.is_some() {
            return Err("A calibration sweep is running.".into());
        }
    }
    state::call(&app, "fan.force", json!({ "index": index, "percent": percent.min(100) })).await.map_err(err)?;
    state.overrides.lock().unwrap().manual.insert(index, percent.min(100));
    state::emit_overrides(&app);
    Ok(())
}

#[tauri::command]
pub async fn release_fans(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if state.overrides.lock().unwrap().failsafe {
        return Err("The thermal failsafe is active; it releases automatically once temperatures recover.".into());
    }
    state::call(&app, "fan.release", json!({})).await.map_err(err)?;
    state.overrides.lock().unwrap().manual.clear();
    state::emit_overrides(&app);
    Ok(())
}

#[tauri::command]
pub fn get_overrides(state: State<'_, AppState>) -> Overrides {
    state.overrides.lock().unwrap().clone()
}

#[tauri::command]
pub async fn calibrate_fan(app: AppHandle, index: usize) -> CmdResult<Vec<[f64; 2]>> {
    let state = app.state::<AppState>();
    {
        let mut overrides = state.overrides.lock().unwrap();
        if overrides.failsafe {
            return Err("The thermal failsafe is active.".into());
        }
        if overrides.calibrating.is_some() {
            return Err("A calibration sweep is already running.".into());
        }
        overrides.calibrating = Some(index);
    }
    state.cancel_calibration.store(false, Ordering::SeqCst);
    state::emit_overrides(&app);

    let result = sweep(&app, index).await;

    let _ = state::call(&app, "fan.release", json!({})).await;
    {
        let mut overrides = state.overrides.lock().unwrap();
        overrides.calibrating = None;
        overrides.manual.clear();
    }
    state::emit_overrides(&app);

    let points = result?;
    state.settings.lock().unwrap().calibration.insert(index, points.clone());
    state.persist_settings()?;
    emit_settings(&app);
    Ok(points)
}

#[tauri::command]
pub fn cancel_calibration(state: State<'_, AppState>) {
    state::request_calibration_cancel(&state);
}

async fn sweep(app: &AppHandle, index: usize) -> CmdResult<Vec<[f64; 2]>> {
    // Descending, so the stop threshold is measured from a spinning fan.
    const STEPS: [u8; 11] = [100, 90, 80, 70, 60, 50, 40, 30, 20, 10, 0];
    let state = app.state::<AppState>();
    let mut points = Vec::with_capacity(STEPS.len());

    for (step, &percent) in STEPS.iter().enumerate() {
        state::call(app, "fan.force", json!({ "index": index, "percent": percent })).await.map_err(err)?;

        let settle_ms = if step == 0 { 6000 } else { 3500 };
        for _ in 0..settle_ms / 250 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if state.cancel_calibration.load(Ordering::SeqCst) {
                return Err("Calibration cancelled.".into());
            }
        }

        let mut rpm_total = 0.0;
        for read in 0..2 {
            if read > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let raw = state::call(app, "sensors", json!({})).await.map_err(err)?;
            let cpu_hot = raw["sensors"].as_array().into_iter().flatten().any(|s| {
                s["type"] == "Temperature" && s["title"] == "CPU" && s["value"].as_f64().unwrap_or(0.0) as f32 > CALIBRATION_CPU_LIMIT
            });
            if cpu_hot {
                return Err(format!("Stopped: CPU temperature exceeded {CALIBRATION_CPU_LIMIT}°C."));
            }
            rpm_total += raw["headerRpms"][index].as_f64().unwrap_or(0.0);
        }
        let rpm = (rpm_total / 2.0).round();
        points.push([percent as f64, rpm]);
        let _ = app.emit(
            "calibration",
            json!({ "index": index, "step": step + 1, "total": STEPS.len(), "percent": percent, "rpm": rpm }),
        );
    }

    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(points)
}

// ---- monitoring -------------------------------------------------------------

#[tauri::command]
pub fn get_latest(state: State<'_, AppState>) -> Option<Sample> {
    state.monitor.lock().unwrap().latest()
}

#[tauri::command]
pub fn get_history(state: State<'_, AppState>, keys: Option<Vec<String>>, max_points: Option<usize>) -> Value {
    state.monitor.lock().unwrap().history(keys.as_deref(), max_points.unwrap_or(900))
}

#[tauri::command]
pub fn reset_stats(state: State<'_, AppState>) {
    state.monitor.lock().unwrap().reset_stats();
}

fn resolved_log_dir(state: &AppState) -> PathBuf {
    let settings = state.settings.lock().unwrap();
    settings.log_dir.clone().map(PathBuf::from).unwrap_or_else(|| state.data_dir.join("logs"))
}

#[tauri::command]
pub fn log_dir(state: State<'_, AppState>) -> String {
    resolved_log_dir(&state).display().to_string()
}

#[tauri::command]
pub fn open_log_dir(app: AppHandle) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = resolved_log_dir(&app.state::<AppState>());
    std::fs::create_dir_all(&dir).map_err(err)?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(err)
}

// ---- profiles ---------------------------------------------------------------

fn emit_profiles(app: &AppHandle) {
    let state = app.state::<AppState>();
    let profiles = state.profiles.lock().unwrap().clone();
    let _ = app.emit("profiles", profiles);
}

fn emit_settings(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let _ = app.emit("settings", settings);
}

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Vec<Profile> {
    state.profiles.lock().unwrap().clone()
}

#[tauri::command]
pub async fn save_profile(app: AppHandle, name: String, fans: Option<Vec<FanCurve>>) -> CmdResult<Profile> {
    let mut fans = match fans {
        Some(f) => f,
        None => get_fans(app.clone()).await?,
    };
    fans.iter_mut().for_each(|f| f.verified = None);
    let name = name.trim();
    let profile = Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: if name.is_empty() { "Untitled".into() } else { name.into() },
        created_at: now_ms(),
        source: None,
        fans,
    };
    let state = app.state::<AppState>();
    state.profiles.lock().unwrap().push(profile.clone());
    state.persist_profiles()?;
    state.settings.lock().unwrap().active_profile = Some(profile.id.clone());
    state.persist_settings()?;
    emit_profiles(&app);
    emit_settings(&app);
    Ok(profile)
}

#[tauri::command]
pub fn rename_profile(app: AppHandle, id: String, name: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    {
        let mut profiles = state.profiles.lock().unwrap();
        let profile = profiles.iter_mut().find(|p| p.id == id).ok_or("Profile not found")?;
        profile.name = name.trim().to_owned();
    }
    state.persist_profiles()?;
    emit_profiles(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_profile(app: AppHandle, id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    state.profiles.lock().unwrap().retain(|p| p.id != id);
    {
        let mut settings = state.settings.lock().unwrap();
        if settings.active_profile.as_deref() == Some(&id) {
            settings.active_profile = None;
        }
        if settings.apply_profile_on_start.as_deref() == Some(&id) {
            settings.apply_profile_on_start = None;
        }
    }
    state.persist_profiles()?;
    state.persist_settings()?;
    emit_profiles(&app);
    emit_settings(&app);
    Ok(())
}

pub async fn apply_profile_inner(app: &AppHandle, id: &str) -> CmdResult<Vec<FanCurve>> {
    let state = app.state::<AppState>();
    let profile = state.profiles.lock().unwrap().iter().find(|p| p.id == id).cloned().ok_or("Profile not found")?;
    state::connection(app).await.map_err(err)?;
    let count = fan_count(&state);

    let mut written = Vec::new();
    for curve in profile.fans.iter().filter(|c| c.index < count) {
        written.push(write_curve(app, curve).await?);
    }
    state.settings.lock().unwrap().active_profile = Some(profile.id.clone());
    state.persist_settings()?;
    emit_settings(app);
    let _ = app.emit("fans", &written);
    Ok(written)
}

#[tauri::command]
pub async fn apply_profile(app: AppHandle, id: String) -> CmdResult<Vec<FanCurve>> {
    apply_profile_inner(&app, &id).await
}

#[tauri::command]
pub fn import_gcc(app: AppHandle) -> CmdResult<Vec<Profile>> {
    let imported = import_gcc_profiles(&PathBuf::from(GCC_FAN_PROFILE_DIR), now_ms())?;
    if imported.is_empty() {
        return Err("No GIGABYTE Control Center fan profiles were found.".into());
    }
    let state = app.state::<AppState>();
    {
        let mut profiles = state.profiles.lock().unwrap();
        profiles.retain(|p| !imported.iter().any(|i| i.id == p.id));
        profiles.extend(imported.iter().cloned());
    }
    state.persist_profiles()?;
    emit_profiles(&app);
    Ok(imported)
}

#[tauri::command]
pub fn export_profile(state: State<'_, AppState>, id: String, path: String) -> CmdResult<()> {
    let profile = state.profiles.lock().unwrap().iter().find(|p| p.id == id).cloned().ok_or("Profile not found")?;
    std::fs::write(path, serde_json::to_vec_pretty(&profile).map_err(err)?).map_err(err)
}

#[tauri::command]
pub fn import_profile_file(app: AppHandle, path: String) -> CmdResult<Vec<Profile>> {
    let bytes = std::fs::read(&path).map_err(err)?;
    let mut incoming: Vec<Profile> = serde_json::from_slice::<Vec<Profile>>(&bytes)
        .or_else(|_| serde_json::from_slice::<Profile>(&bytes).map(|p| vec![p]))
        .map_err(|e| format!("Not a Helm profile file: {e}"))?;
    for p in &mut incoming {
        p.id = uuid::Uuid::new_v4().to_string();
        p.source = Some("file".into());
    }
    let state = app.state::<AppState>();
    state.profiles.lock().unwrap().extend(incoming.iter().cloned());
    state.persist_profiles()?;
    emit_profiles(&app);
    Ok(incoming)
}

// ---- settings ---------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    let mut settings = state.settings.lock().unwrap().clone();
    settings.start_with_windows = system::autostart_exists();
    settings
}

#[tauri::command]
pub fn update_settings(app: AppHandle, settings: Settings) -> CmdResult<Settings> {
    let state = app.state::<AppState>();
    let settings = settings.sanitized();

    if settings.start_with_windows != system::autostart_exists() {
        system::set_autostart(settings.start_with_windows)?;
    }

    *state.settings.lock().unwrap() = settings.clone();
    state.persist_settings()?;
    state.wake.notify_one();
    emit_settings(&app);
    Ok(settings)
}

// ---- board ------------------------------------------------------------------

#[tauri::command]
pub async fn get_board_info(app: AppHandle) -> CmdResult<Value> {
    state::call(&app, "board.fwinfo", json!({})).await.map_err(err)
}

// ---- lighting ---------------------------------------------------------------

#[tauri::command]
pub async fn rgb_get(app: AppHandle) -> CmdResult<Value> {
    state::call(&app, "rgb.get", json!({})).await.map_err(err)
}

#[tauri::command]
pub async fn rgb_set(app: AppHandle, zones: Vec<LightingZone>) -> CmdResult<Value> {
    let result = state::call(&app, "rgb.set", json!({ "zones": zones })).await.map_err(err)?;
    let state = app.state::<AppState>();
    state.settings.lock().unwrap().lighting = Some(zones);
    state.persist_settings()?;
    emit_settings(&app);
    Ok(result)
}

pub async fn restore_lighting(app: &AppHandle) {
    let state = app.state::<AppState>();
    let zones = {
        let settings = state.settings.lock().unwrap();
        if !settings.restore_lighting {
            return;
        }
        settings.lighting.clone()
    };
    let Some(zones) = zones.filter(|z| !z.is_empty()) else { return };
    if let Err(e) = state::call(app, "rgb.set", json!({ "zones": zones })).await {
        if e.code != "rgb_unavailable" {
            let _ = app.emit("toast", json!({ "kind": "error", "message": format!("Couldn't restore lighting: {e}") }));
        }
    }
}

#[tauri::command]
pub fn save_lighting_preset(app: AppHandle, name: String, zones: Vec<LightingZone>) -> CmdResult<LightingPreset> {
    let name = name.trim();
    let preset = LightingPreset {
        id: uuid::Uuid::new_v4().to_string(),
        name: if name.is_empty() { "Untitled".into() } else { name.into() },
        zones,
    };
    let state = app.state::<AppState>();
    state.settings.lock().unwrap().lighting_presets.push(preset.clone());
    state.persist_settings()?;
    emit_settings(&app);
    Ok(preset)
}

#[tauri::command]
pub fn delete_lighting_preset(app: AppHandle, id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    state.settings.lock().unwrap().lighting_presets.retain(|p| p.id != id);
    state.persist_settings()?;
    emit_settings(&app);
    Ok(())
}

// ---- overclocking -----------------------------------------------------------

/// Live overclock changes revert on their own unless confirmed within this window, so a setting
/// that hangs the display or the UI can't stick.
const OC_CONFIRM_SECS: u64 = 20;

static OC_PENDING: Mutex<Option<u64>> = Mutex::new(None);
static OC_TOKEN: AtomicU64 = AtomicU64::new(0);

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcChange {
    app_id: i64,
    value: i64,
}

fn live_deviation(result: &Value) -> bool {
    result["items"].as_array().into_iter().flatten().any(|i| i["live"] == true && i["current"] != i["boot"])
}

#[tauri::command]
pub async fn oc_get(app: AppHandle) -> CmdResult<Value> {
    let mut data = state::call(&app, "oc.get", json!({})).await.map_err(err)?;
    data["pendingConfirm"] = OC_PENDING.lock().unwrap().is_some().into();
    Ok(data)
}

#[tauri::command]
pub async fn oc_apply(app: AppHandle, changes: Vec<OcChange>, allow_reboot: bool) -> CmdResult<Value> {
    if changes.is_empty() {
        return Err("Nothing to apply.".into());
    }
    let mut result = state::call(&app, "oc.apply", json!({ "changes": changes, "allowReboot": allow_reboot })).await.map_err(err)?;

    if live_deviation(&result) {
        let token = OC_TOKEN.fetch_add(1, Ordering::SeqCst) + 1;
        *OC_PENDING.lock().unwrap() = Some(token);
        result["revertInSecs"] = OC_CONFIRM_SECS.into();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(OC_CONFIRM_SECS)).await;
            let expired = {
                let mut pending = OC_PENDING.lock().unwrap();
                if *pending == Some(token) {
                    *pending = None;
                    true
                } else {
                    false
                }
            };
            if expired {
                let outcome = state::call(&app, "oc.restore", json!({})).await;
                let _ = app.emit("oc-reverted", json!({ "ok": outcome.is_ok(), "error": outcome.err().map(|e| e.to_string()) }));
            }
        });
    }
    Ok(result)
}

#[tauri::command]
pub fn oc_confirm() -> bool {
    OC_PENDING.lock().unwrap().take().is_some()
}

#[tauri::command]
pub async fn oc_restore(app: AppHandle) -> CmdResult<Value> {
    OC_PENDING.lock().unwrap().take();
    state::call(&app, "oc.restore", json!({})).await.map_err(err)
}

// ---- storage ----------------------------------------------------------------

#[tauri::command]
pub async fn storage_scan() -> CmdResult<Vec<storage::Disk>> {
    tokio::task::spawn_blocking(storage::scan).await.map_err(err)
}

/// Sends TRIM for all free space on an NTFS/ReFS volume (`defrag X: /L`), the same as Windows' weekly optimization.
#[tauri::command]
pub async fn storage_trim(letter: String) -> CmdResult<String> {
    let letter = letter.trim().trim_end_matches([':', '\\']).to_ascii_uppercase();
    if letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err("Invalid drive letter.".into());
    }
    let output = tokio::task::spawn_blocking(move || {
        let mut c = std::process::Command::new("defrag");
        c.args([&format!("{letter}:"), "/L", "/U"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        c.output()
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        Ok(text.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("Done.").to_string())
    } else {
        let line = text.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("").to_string();
        Err(if line.is_empty() { format!("Optimization failed (code {:?}).", output.status.code()) } else { line })
    }
}

// ---- updates ----------------------------------------------------------------

#[tauri::command]
pub async fn updates_scan(app: AppHandle) -> CmdResult<updates::DriverScan> {
    updates::scan_drivers(&app).await
}

#[tauri::command]
pub async fn updates_install(app: AppHandle, ids: Vec<i64>) -> CmdResult<Vec<updates::InstallResult>> {
    updates::install_drivers(&app, ids).await
}

#[tauri::command]
pub fn updates_cancel() {
    updates::cancel();
}

#[tauri::command]
pub async fn bios_check(app: AppHandle) -> CmdResult<updates::BiosInfo> {
    updates::check_bios(&app).await
}

#[tauri::command]
pub async fn bios_download(app: AppHandle) -> CmdResult<updates::BiosDownload> {
    updates::download_bios(&app).await
}

#[tauri::command]
pub fn bios_usb_drives() -> Vec<updates::UsbDrive> {
    updates::usb_drives()
}

#[tauri::command]
pub async fn bios_copy_to_usb(letter: String) -> CmdResult<String> {
    tokio::task::spawn_blocking(move || updates::copy_bios_to_usb(&letter)).await.map_err(err)?
}

#[tauri::command]
pub async fn bios_flash_readiness() -> CmdResult<updates::FlashReadiness> {
    tokio::task::spawn_blocking(updates::flash_readiness).await.map_err(err)
}

#[tauri::command]
pub async fn bios_flash(confirm_model: String, suspend_bitlocker: bool) -> CmdResult<()> {
    tokio::task::spawn_blocking(move || updates::flash_bios(&confirm_model, suspend_bitlocker)).await.map_err(err)?
}

#[tauri::command]
pub fn open_downloads(app: AppHandle) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = updates::open_folder_target(&app)?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(err)
}

// ---- graphics ----------------------------------------------------------------

#[tauri::command]
pub fn gpu_get() -> gpu::GpuSnapshot {
    gpu::latest()
}

#[tauri::command]
pub fn gpu_apply(app: AppHandle, change: gpu::GpuApply) -> CmdResult<gpu::GpuSnapshot> {
    gpu::apply(&app, change)
}

#[tauri::command]
pub fn gpu_confirm(app: AppHandle) -> CmdResult<bool> {
    gpu::confirm(&app)
}

#[tauri::command]
pub fn gpu_restore(app: AppHandle) -> CmdResult<gpu::GpuSnapshot> {
    gpu::restore_clocks(&app)
}

// ---- devices ------------------------------------------------------------------

#[tauri::command]
pub fn devices_get() -> logitech::DevicesSnapshot {
    logitech::latest()
}

#[tauri::command]
pub fn devices_apply(app: AppHandle, change: logitech::DeviceApply) -> CmdResult<logitech::DevicesSnapshot> {
    logitech::apply(&app, change)
}
