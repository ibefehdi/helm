use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// One fan header's curve exactly as the engine stores it: up to 8 (°C, PWM 0-255) points.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FanCurve {
    pub index: usize,
    pub mode: String,
    pub hysteresis: i32,
    #[serde(default)]
    pub fan_speed: i32,
    pub temperature_source: i32,
    pub points: Vec<[f64; 2]>,
    /// Set by the bridge after a write: whether the read-back matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    #[serde(default)]
    pub source: Option<String>,
    pub fans: Vec<FanCurve>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AlertOp {
    Above,
    Below,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AlertAction {
    Notify,
    /// Pin every header at 100% until the reading recovers.
    FullSpeed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: String,
    pub sensor: String,
    pub op: AlertOp,
    pub threshold: f64,
    #[serde(default = "default_alert_hysteresis")]
    pub hysteresis: f64,
    pub action: AlertAction,
    pub enabled: bool,
}

fn default_alert_hysteresis() -> f64 {
    3.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub poll_ms: u64,
    pub history_minutes: u32,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub start_with_windows: bool,
    pub apply_profile_on_start: Option<String>,
    pub active_profile: Option<String>,
    pub log_csv: bool,
    pub log_dir: Option<String>,
    pub alerts: Vec<Alert>,
    /// User-facing names for headers, keyed by header index.
    pub fan_labels: BTreeMap<usize, String>,
    /// Measured (duty %, RPM) pairs per header from a calibration sweep.
    pub calibration: BTreeMap<usize, Vec<[f64; 2]>>,
    pub hidden_sensors: Vec<String>,
    pub pinned_sensors: Vec<String>,
    /// Last lighting written by Helm, re-applied at startup and after resume when `restore_lighting` is on.
    pub lighting: Option<Vec<LightingZone>>,
    pub restore_lighting: bool,
    /// Lighting presets saved by the user.
    pub lighting_presets: Vec<LightingPreset>,
    /// NVIDIA graphics tuning. Applied by Helm through the driver's NVML interface.
    pub gpu: GpuControl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LightingZone {
    pub index: u32,
    pub mode: u32,
    pub color: u32,
    pub speed: u8,
    pub brightness: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightingPreset {
    pub id: String,
    pub name: String,
    pub zones: Vec<LightingZone>,
}

/// Persisted graphics controls. `fan_mode` is `auto`, `manual` or `curve`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GpuControl {
    pub core_offset: i32,
    pub memory_offset: i32,
    pub power_limit_w: Option<f64>,
    pub temp_limit_c: Option<u32>,
    pub fan_mode: String,
    pub fan_percent: u32,
    pub fan_curve: Vec<[f64; 2]>,
    pub apply_on_start: bool,
    pub osd: bool,
}

impl Default for GpuControl {
    fn default() -> Self {
        Self {
            core_offset: 0,
            memory_offset: 0,
            power_limit_w: None,
            temp_limit_c: None,
            fan_mode: "auto".into(),
            fan_percent: 40,
            fan_curve: vec![[45.0, 30.0], [60.0, 45.0], [72.0, 70.0], [82.0, 100.0]],
            apply_on_start: false,
            osd: false,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_ms: 1000,
            history_minutes: 15,
            close_to_tray: true,
            start_minimized: false,
            start_with_windows: false,
            apply_profile_on_start: None,
            active_profile: None,
            log_csv: false,
            log_dir: None,
            alerts: vec![
                Alert {
                    id: "default-cpu-hot".into(),
                    sensor: "Temperature/CPU".into(),
                    op: AlertOp::Above,
                    threshold: 90.0,
                    hysteresis: 5.0,
                    action: AlertAction::FullSpeed,
                    enabled: true,
                },
                Alert {
                    id: "default-cpu-fan-stall".into(),
                    sensor: "Fan/CPU".into(),
                    op: AlertOp::Below,
                    threshold: 200.0,
                    hysteresis: 100.0,
                    action: AlertAction::Notify,
                    enabled: true,
                },
            ],
            fan_labels: BTreeMap::new(),
            calibration: BTreeMap::new(),
            hidden_sensors: Vec::new(),
            pinned_sensors: vec!["Temperature/CPU".into(), "Fan/CPU".into(), "Voltage/CPU Vcore".into()],
            lighting: None,
            restore_lighting: true,
            lighting_presets: Vec::new(),
            gpu: GpuControl::default(),
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.poll_ms = self.poll_ms.clamp(250, 10_000);
        self.history_minutes = self.history_minutes.clamp(1, 240);
        if !matches!(self.gpu.fan_mode.as_str(), "auto" | "manual" | "curve") {
            self.gpu.fan_mode = "auto".into();
        }
        self.gpu.fan_percent = self.gpu.fan_percent.min(100);
        self.gpu.fan_curve = sanitize_fan_curve(self.gpu.fan_curve);
        self
    }
}

fn sanitize_fan_curve(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.retain(|p| p[0].is_finite() && p[1].is_finite());
    for p in &mut points {
        p[0] = p[0].clamp(0.0, 110.0);
        p[1] = p[1].clamp(0.0, 100.0);
    }
    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
    points.dedup_by(|a, b| (a[0] - b[0]).abs() < 0.5);
    if points.len() < 2 {
        GpuControl::default().fan_curve
    } else {
        points.truncate(8);
        points
    }
}

pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(tmp, path)
}

/// Reads GCC's own fan profiles (`Control Center\Fan\Profile-N.xml` + `index.xml`).
/// Header order in GCC's XML matches the engine's fan-control index order.
pub fn import_gcc_profiles(fan_dir: &Path, now: i64) -> Result<Vec<Profile>, String> {
    let index = fs::read_to_string(fan_dir.join("index.xml")).map_err(|e| format!("index.xml: {e}"))?;
    let index = roxmltree::Document::parse(&index).map_err(|e| format!("index.xml: {e}"))?;

    let mut profiles = Vec::new();
    for entry in index.descendants().filter(|n| n.has_tag_name("ICFileInfoIndex")) {
        let text = |tag: &str| entry.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).unwrap_or("");
        if text("IsDummy").eq_ignore_ascii_case("true") {
            continue;
        }
        let key = text("KeyIndex");
        let name = match text("Description") {
            "" => format!("GCC profile {key}"),
            d => format!("GCC · {d}"),
        };

        let path = fan_dir.join(format!("Profile-{key}.xml"));
        let xml = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let fans = parse_gcc_fan_profile(&xml).map_err(|e| format!("{}: {e}", path.display()))?;
        if fans.is_empty() {
            continue;
        }
        profiles.push(Profile {
            id: format!("gcc-{key}"),
            name,
            created_at: now,
            source: Some("gcc".into()),
            fans,
        });
    }
    Ok(profiles)
}

fn parse_gcc_fan_profile(xml: &str) -> Result<Vec<FanCurve>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let child_text = |node: roxmltree::Node, tag: &str| -> Option<String> {
        node.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).map(str::to_owned)
    };

    let mut fans = Vec::new();
    for (index, config) in doc.descendants().filter(|n| n.has_tag_name("SmartFanDataPointConfig")).enumerate() {
        let mode = child_text(config, "Mode").unwrap_or_else(|| "SmartFanMode".into());
        let temperature_source = child_text(config, "TemperatureInputSelection").and_then(|s| s.parse().ok()).unwrap_or(0);

        let Some(set) = config
            .descendants()
            .filter(|n| n.has_tag_name("SmartFanDataPointSet"))
            .find(|s| child_text(*s, "Mode").as_deref() == Some(mode.as_str()))
        else {
            continue;
        };

        let points = set
            .descendants()
            .filter(|n| n.has_tag_name("SmartFanDataPoint"))
            .filter_map(|p| {
                let x = child_text(p, "X")?.parse().ok()?;
                let y = child_text(p, "Y")?.parse().ok()?;
                Some([x, y])
            })
            .collect::<Vec<_>>();

        fans.push(FanCurve {
            index,
            mode,
            hysteresis: child_text(set, "DeltaTemperature").and_then(|s| s.parse().ok()).unwrap_or(1),
            fan_speed: child_text(set, "FanSpeed").and_then(|s| s.parse().ok()).unwrap_or(0),
            temperature_source,
            points,
            verified: None,
        });
    }
    Ok(fans)
}
