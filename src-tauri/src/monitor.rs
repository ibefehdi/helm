use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::model::{AlertAction, AlertOp, Settings};
use crate::state::{self, AppState};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub key: String,
    pub kind: String,
    pub title: String,
    pub unit: String,
    pub value: Option<f32>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub avg: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub t: i64,
    pub readings: Vec<Reading>,
    pub header_rpms: Vec<f32>,
    pub header_temperatures: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertEvent {
    pub id: String,
    pub sensor: String,
    pub active: bool,
    pub value: f32,
    pub threshold: f64,
    pub action: AlertAction,
}

struct Series {
    points: VecDeque<(i64, f32)>,
    min: f32,
    max: f32,
    sum: f64,
    count: u64,
}

impl Series {
    fn new() -> Self {
        Self { points: VecDeque::new(), min: f32::INFINITY, max: f32::NEG_INFINITY, sum: 0.0, count: 0 }
    }

    fn push(&mut self, t: i64, v: f32, horizon_ms: i64) {
        self.points.push_back((t, v));
        while self.points.front().is_some_and(|(pt, _)| t - pt > horizon_ms) {
            self.points.pop_front();
        }
        self.min = self.min.min(v);
        self.max = self.max.max(v);
        self.sum += v as f64;
        self.count += 1;
    }
}

struct CsvLog {
    path: PathBuf,
    keys: Vec<String>,
    file: File,
}

#[derive(Default)]
pub struct Monitor {
    series: BTreeMap<String, Series>,
    latest: Option<Sample>,
    alert_active: HashMap<String, bool>,
    csv: Option<CsvLog>,
}

impl Monitor {
    pub fn latest(&self) -> Option<Sample> {
        self.latest.clone()
    }

    pub fn reset_stats(&mut self) {
        for s in self.series.values_mut() {
            s.min = f32::INFINITY;
            s.max = f32::NEG_INFINITY;
            s.sum = 0.0;
            s.count = 0;
        }
    }

    pub fn history(&self, keys: Option<&[String]>, max_points: usize) -> Value {
        let mut out = serde_json::Map::new();
        for (key, series) in &self.series {
            if keys.is_some_and(|k| !k.contains(key)) {
                continue;
            }
            let step = (series.points.len() / max_points.max(1)).max(1);
            let pts: Vec<[f64; 2]> =
                series.points.iter().step_by(step).map(|(t, v)| [*t as f64, *v as f64]).collect();
            out.insert(key.clone(), json!(pts));
        }
        Value::Object(out)
    }

    fn ingest(&mut self, raw: &Value, horizon_ms: i64) -> Sample {
        let t = raw["timestamp"].as_i64().unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        let mut readings = Vec::new();

        for s in raw["sensors"].as_array().into_iter().flatten() {
            let kind = s["type"].as_str().unwrap_or("Unknown").to_owned();
            let title = s["title"].as_str().unwrap_or("").trim().to_owned();
            let key = format!("{kind}/{title}");
            let unit = normalize_unit(&kind, s["unit"].as_str().unwrap_or(""));
            let value = s["value"].as_f64().map(|v| v as f32);

            let series = self.series.entry(key.clone()).or_insert_with(Series::new);
            if let Some(v) = value {
                series.push(t, v, horizon_ms);
            }
            readings.push(Reading {
                key,
                kind,
                title,
                unit,
                value,
                min: series.min.is_finite().then_some(series.min),
                max: series.max.is_finite().then_some(series.max),
                avg: (series.count > 0).then(|| (series.sum / series.count as f64) as f32),
            });
        }

        let floats = |v: &Value| -> Vec<f32> {
            v.as_array().into_iter().flatten().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()
        };
        let sample = Sample {
            t,
            readings,
            header_rpms: floats(&raw["headerRpms"]),
            header_temperatures: floats(&raw["headerTemperatures"]),
        };
        self.latest = Some(sample.clone());
        sample
    }

    fn evaluate_alerts(&mut self, settings: &Settings, sample: &Sample) -> Vec<AlertEvent> {
        let mut events = Vec::new();
        for alert in settings.alerts.iter().filter(|a| a.enabled) {
            let Some(value) = sample.readings.iter().find(|r| r.key == alert.sensor).and_then(|r| r.value) else {
                continue;
            };
            let v = value as f64;
            let was_active = self.alert_active.get(&alert.id).copied().unwrap_or(false);
            let active = match (alert.op, was_active) {
                (AlertOp::Above, false) => v >= alert.threshold,
                (AlertOp::Above, true) => v > alert.threshold - alert.hysteresis,
                (AlertOp::Below, false) => v <= alert.threshold,
                (AlertOp::Below, true) => v < alert.threshold + alert.hysteresis,
            };
            if active != was_active {
                self.alert_active.insert(alert.id.clone(), active);
                events.push(AlertEvent {
                    id: alert.id.clone(),
                    sensor: alert.sensor.clone(),
                    active,
                    value,
                    threshold: alert.threshold,
                    action: alert.action,
                });
            }
        }
        self.alert_active.retain(|id, _| settings.alerts.iter().any(|a| &a.id == id && a.enabled));
        events
    }

    fn any_failsafe_active(&self, settings: &Settings) -> bool {
        settings
            .alerts
            .iter()
            .any(|a| a.enabled && a.action == AlertAction::FullSpeed && self.alert_active.get(&a.id) == Some(&true))
    }

    fn log_csv(&mut self, dir: PathBuf, sample: &Sample) -> std::io::Result<()> {
        let keys: Vec<String> = sample.readings.iter().map(|r| r.key.clone()).collect();
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        let wanted = dir.join(format!("helm-{day}.csv"));

        let reopen = match &self.csv {
            Some(log) => log.keys != keys || !log.path.starts_with(&dir) || !log.path.to_string_lossy().contains(&day),
            None => true,
        };
        if reopen {
            std::fs::create_dir_all(&dir)?;
            // A sensor list that changed mid-day gets its own file so columns never shift.
            let path = match &self.csv {
                Some(log) if log.path == wanted && log.keys != keys => {
                    dir.join(format!("helm-{day}-{}.csv", chrono::Local::now().format("%H%M%S")))
                }
                _ => wanted,
            };
            let is_new = !path.exists();
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            if is_new {
                let header: Vec<String> = keys.iter().map(|k| csv_escape(k)).collect();
                writeln!(file, "timestamp,{}", header.join(","))?;
            }
            self.csv = Some(CsvLog { path, keys, file });
        }

        let log = self.csv.as_mut().expect("opened above");
        let time = chrono::DateTime::from_timestamp_millis(sample.t)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%dT%H:%M:%S%.3f").to_string())
            .unwrap_or_default();
        let values: Vec<String> =
            sample.readings.iter().map(|r| r.value.map(|v| format!("{v}")).unwrap_or_default()).collect();
        writeln!(log.file, "{time},{}", values.join(","))
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

fn normalize_unit(kind: &str, unit: &str) -> String {
    match kind {
        "Temperature" => "°C".into(),
        "Fan" => "RPM".into(),
        _ => unit.trim().to_owned(),
    }
}

pub async fn run(app: AppHandle) {
    let mut last_tick = chrono::Utc::now();
    loop {
        let state = app.state::<AppState>();
        let poll = Duration::from_millis(state.settings.lock().unwrap().poll_ms);

        // The tokio timer doesn't advance during sleep, so a large wall-clock jump means the PC resumed.
        // The RGB controller loses its state over S3/S4, the same reason GCC re-applies on resume.
        let now = chrono::Utc::now();
        if (now - last_tick).num_milliseconds() > poll.as_millis() as i64 + 30_000 {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { crate::commands::restore_lighting(&app).await });
        }
        last_tick = now;

        let delay = match state::call(&app, "sensors", json!({})).await {
            Ok(raw) => {
                on_sample(&app, &raw).await;
                poll
            }
            Err(_) => Duration::from_secs(5).max(poll),
        };

        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = state.wake.notified() => {}
        }
    }
}

async fn on_sample(app: &AppHandle, raw: &Value) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let horizon_ms = settings.history_minutes as i64 * 60_000;

    let (sample, alerts, failsafe_wanted) = {
        let mut monitor = state.monitor.lock().unwrap();
        let sample = monitor.ingest(raw, horizon_ms);
        let alerts = monitor.evaluate_alerts(&settings, &sample);
        if settings.log_csv {
            let dir = settings.log_dir.clone().map(PathBuf::from).unwrap_or_else(|| state.data_dir.join("logs"));
            if let Err(e) = monitor.log_csv(dir, &sample) {
                let _ = app.emit("toast", json!({ "kind": "error", "message": format!("CSV logging failed: {e}") }));
            }
        }
        let failsafe_wanted = monitor.any_failsafe_active(&settings);
        (sample, alerts, failsafe_wanted)
    };

    let _ = app.emit("sample", &sample);
    update_tray_tooltip(app, &sample);

    for event in &alerts {
        if event.active {
            let verb = if event.action == AlertAction::FullSpeed { " — all fans pinned to 100%" } else { "" };
            let _ = app
                .notification()
                .builder()
                .title("Helm alert")
                .body(format!("{} is {:.1} (limit {:.1}){verb}", event.sensor, event.value, event.threshold))
                .show();
        }
        let _ = app.emit("alert", event);
    }

    sync_failsafe(app, failsafe_wanted).await;

    if !state.startup_profile_applied.swap(true, Ordering::SeqCst) {
        if let Some(id) = settings.apply_profile_on_start.clone() {
            if let Err(e) = crate::commands::apply_profile_inner(app, &id).await {
                let _ = app.emit("toast", json!({ "kind": "error", "message": format!("Startup profile failed: {e}") }));
            }
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move { crate::commands::restore_lighting(&app).await });
    }
}

async fn sync_failsafe(app: &AppHandle, wanted: bool) {
    let state = app.state::<AppState>();
    let currently = state.overrides.lock().unwrap().failsafe;
    if wanted == currently {
        return;
    }

    if wanted {
        state::request_calibration_cancel(&state);
        let fan_count = state.info.lock().unwrap().as_ref().and_then(|i| i["fanCount"].as_u64()).unwrap_or(0);
        for index in 0..fan_count {
            let _ = state::call(app, "fan.force", json!({ "index": index, "percent": 100 })).await;
        }
    } else {
        let _ = state::call(app, "fan.release", json!({})).await;
    }

    {
        let mut overrides = state.overrides.lock().unwrap();
        overrides.failsafe = wanted;
        overrides.manual.clear();
    }
    state::emit_overrides(app);
}

fn update_tray_tooltip(app: &AppHandle, sample: &Sample) {
    let Some(tray) = app.tray_by_id("main") else { return };
    let find = |key: &str| sample.readings.iter().find(|r| r.key == key).and_then(|r| r.value);
    let mut parts = vec!["Helm".to_owned()];
    if let Some(cpu) = find("Temperature/CPU") {
        parts.push(format!("CPU {cpu:.0}°C"));
    }
    if let Some(rpm) = find("Fan/CPU") {
        parts.push(format!("{rpm:.0} RPM"));
    }
    let _ = tray.set_tooltip(Some(parts.join(" · ")));
}
