//! Logitech device settings through the local G HUB agent.
//!
//! The G HUB window sends JSON to `ws://127.0.0.1:9010` (subprotocol `json`):
//! `{ msgId, verb, path, payload }`. Verbs are GET and SET. Paths and payload fields
//! come from the client's own protocol definitions. Helm does not load G HUB's binaries.

use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{ClientRequestBuilder, Message, connect};

const AGENT: &str = "ws://127.0.0.1:9010";

static SNAP: Mutex<Option<DevicesSnapshot>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesSnapshot {
    pub agent: bool,
    pub message: Option<String>,
    pub devices: Vec<LogiDevice>,
    pub known: Vec<KnownDevice>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogiDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub model: String,
    pub connection: String,
    pub state: String,
    pub battery: Option<Battery>,
    pub mouse: Option<Mouse>,
    pub audio: Option<Audio>,
    pub lighting: Option<Lighting>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownDevice {
    pub model: String,
    pub name: String,
    pub kind: String,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: Option<u32>,
    pub charging: Option<bool>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mouse {
    pub active_dpi: u32,
    pub default_dpi: u32,
    pub levels: Vec<u32>,
    pub report_rate: Option<u32>,
    pub report_rates: Vec<u32>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Audio {
    pub volume: Option<u32>,
    pub muted: Option<bool>,
    pub sidetone: Option<u32>,
    pub sidetone_muted: Option<bool>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lighting {
    pub on: Option<bool>,
    pub brightness: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceApply {
    pub id: String,
    pub active_dpi: Option<u32>,
    pub dpi_levels: Option<Vec<u32>>,
    pub report_rate: Option<u32>,
    pub volume: Option<u32>,
    pub muted: Option<bool>,
    pub sidetone: Option<u32>,
    pub sidetone_muted: Option<bool>,
    pub lighting_on: Option<bool>,
    pub brightness: Option<u32>,
}

struct Agent {
    socket: tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
}

impl Agent {
    fn connect() -> Result<Self, String> {
        let uri = AGENT.parse().map_err(|e| format!("Bad agent address: {e}"))?;
        let request = ClientRequestBuilder::new(uri).with_sub_protocol("json");
        let (mut socket, _) = connect(request).map_err(|_| {
            "The G HUB agent isn't running. Start Logitech G HUB and Helm can use its devices.".to_string()
        })?;
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
            stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
        }
        // The agent greets with OPTIONS / before it answers requests.
        let _ = recv_text(&mut socket);
        Ok(Self { socket })
    }

    fn call(&mut self, verb: &str, path: &str, payload: Option<Value>) -> Result<Value, String> {
        let msg_id = NEXT_ID.fetch_add(1, Ordering::Relaxed).to_string();
        let mut body = json!({ "msgId": msg_id, "verb": verb, "path": path });
        if let Some(payload) = payload {
            body["payload"] = payload;
        }
        self.socket
            .send(Message::Text(body.to_string().into()))
            .map_err(|e| format!("Couldn't reach the G HUB agent: {e}"))?;
        for _ in 0..12 {
            let text = recv_text(&mut self.socket)?;
            let value: Value = serde_json::from_str(&text).map_err(|e| format!("The agent sent an unreadable reply: {e}"))?;
            if value.get("msgId").and_then(|id| id.as_str()) != Some(msg_id.as_str()) {
                continue;
            }
            let code = value["result"]["code"].as_str().unwrap_or("");
            if code != "SUCCESS" {
                let what = value["result"]["what"].as_str().unwrap_or(code);
                return Err(if what.is_empty() { format!("The agent refused {path} ({code}).") } else { what.to_string() });
            }
            return Ok(value.get("payload").cloned().unwrap_or(Value::Null));
        }
        Err(format!("The agent never answered {path}."))
    }

    fn get(&mut self, path: &str) -> Option<Value> {
        match self.call("GET", path, None) {
            Ok(value) if !value.is_null() => Some(value),
            _ => None,
        }
    }
}

fn recv_text(socket: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>) -> Result<String, String> {
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => return Ok(text.to_string()),
            Ok(Message::Ping(payload)) => {
                let _ = socket.send(Message::Pong(payload));
            }
            Ok(Message::Close(_)) => return Err("The G HUB agent closed the connection.".into()),
            Ok(_) => {}
            Err(e) => return Err(format!("Couldn't reach the G HUB agent: {e}")),
        }
    }
}

pub fn latest() -> DevicesSnapshot {
    SNAP.lock().unwrap().clone().unwrap_or(DevicesSnapshot {
        agent: false,
        message: Some("Looking for the G HUB agent…".into()),
        devices: Vec::new(),
        known: Vec::new(),
    })
}

pub fn read() -> DevicesSnapshot {
    let mut agent = match Agent::connect() {
        Ok(agent) => agent,
        Err(message) => {
            return DevicesSnapshot { agent: false, message: Some(message), devices: Vec::new(), known: Vec::new() };
        }
    };
    let list = agent.get("/devices/list");
    let known_payload = agent.get("/devices/list/known");
    let mut devices = Vec::new();
    if let Some(list) = list {
        for info in array_at(&list, &["deviceInfos"]) {
            if let Some(device) = read_device(&mut agent, &info) {
                devices.push(device);
            }
        }
    }
    let connected_models: Vec<String> = devices.iter().map(|d| d.model.to_ascii_lowercase()).collect();
    let known = known_payload
        .map(|payload| {
            array_at(&payload, &["knownList"])
                .into_iter()
                .filter_map(|item| parse_known(&item))
                .filter(|item| !connected_models.iter().any(|model| model == &item.model.to_ascii_lowercase() || model == &item.name.to_ascii_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    DevicesSnapshot { agent: true, message: None, devices, known }
}

fn read_device(agent: &mut Agent, info: &Value) -> Option<LogiDevice> {
    let id = text(info, "id")?;
    if id.is_empty() {
        return None;
    }
    let kind = text(info, "deviceType").unwrap_or_else(|| "DEVICE".into());
    let model = text(info, "deviceBaseModel")
        .filter(|m| !m.is_empty())
        .or_else(|| text(info, "deviceModel"))
        .unwrap_or_default();
    let name = text(info, "givenName")
        .filter(|n| !n.is_empty())
        .or_else(|| text(info, "extendedDisplayName").filter(|n| !n.is_empty()))
        .or_else(|| text(info, "displayName").filter(|n| !n.is_empty()))
        .unwrap_or_else(|| pretty_model(&model, &kind));
    let mouse = if kind_is(&kind, &["MOUSE"]) { read_mouse(agent, &id) } else { None };
    let audio = if kind_is(&kind, &["HEADSET", "SPEAKER", "MICROPHONE"]) { read_audio(agent, &id) } else { None };
    Some(LogiDevice {
        battery: read_battery(agent, &id),
        lighting: read_lighting(agent, &id),
        connection: text(info, "displayConnectionType").filter(|c| !c.starts_with("INVALID")).unwrap_or_else(|| text(info, "connectionType").unwrap_or_default()),
        state: text(info, "state").unwrap_or_default(),
        id,
        name,
        kind: pretty_kind(&kind),
        model,
        mouse,
        audio,
    })
}

fn read_battery(agent: &mut Agent, id: &str) -> Option<Battery> {
    let payload = agent.get(&format!("/battery/{id}/state"))?;
    let percent = number_at(&payload, &["percentage", "percent", "batteryPercent"]).map(|n| n.round().clamp(0.0, 100.0) as u32);
    let charging = bool_at(&payload, &["charging", "isCharging"]);
    if percent.is_none() && charging.is_none() {
        return None;
    }
    Some(Battery { percent, charging })
}

fn read_mouse(agent: &mut Agent, id: &str) -> Option<Mouse> {
    let info = agent.get(&format!("/mouse/{id}/info"))?;
    let table = info.get("dpiTable").or_else(|| info.get("mouseSettings").and_then(|m| m.get("dpiTable"))).unwrap_or(&info);
    let levels = table
        .get("levels")
        .and_then(|v| v.as_array())
        .map(|levels| levels.iter().filter_map(|n| n.as_u64().map(|n| n as u32)).filter(|n| *n > 0).collect::<Vec<_>>())
        .unwrap_or_default();
    let active = table.get("activeDpi").and_then(|n| n.as_u64()).map(|n| n as u32).unwrap_or(0);
    if levels.is_empty() || active == 0 {
        return None;
    }
    let rates_payload = agent.get(&format!("/report_rate/{id}/report_rate_list")).or_else(|| agent.get(&format!("/wireless/{id}/report_rate")));
    let report_rates = rates_payload.as_ref().map(|payload| number_list(payload)).unwrap_or_default();
    let report_rate = info
        .pointer("/mouseSettings/reportRate/value")
        .or_else(|| info.pointer("/reportRate/value"))
        .and_then(|n| n.as_u64())
        .map(|n| n as u32)
        .or_else(|| rates_payload.as_ref().and_then(|p| number_at(p, &["value", "reportRate"]).map(|n| n as u32)));
    Some(Mouse {
        active_dpi: active,
        default_dpi: table.get("defaultDpi").and_then(|n| n.as_u64()).map(|n| n as u32).unwrap_or(active),
        levels,
        report_rate,
        report_rates,
    })
}

fn read_audio(agent: &mut Agent, id: &str) -> Option<Audio> {
    let volume = agent.get(&format!("/audio/{id}/volume"));
    let sidetone = agent.get(&format!("/audio/{id}/sidetone"));
    let output = volume.as_ref().and_then(|payload| channel(payload, &["OUTPUT", "CENTURION_OUTPUT"]));
    let side = sidetone.as_ref().and_then(|payload| channel(payload, &["SIDETONE"])).or_else(|| volume.as_ref().and_then(|payload| channel(payload, &["SIDETONE"])));
    if output.is_none() && side.is_none() {
        return None;
    }
    Some(Audio {
        volume: output.as_ref().map(|c| c.percent),
        muted: output.as_ref().map(|c| c.muted),
        sidetone: side.as_ref().map(|c| c.percent),
        sidetone_muted: side.as_ref().map(|c| c.muted),
    })
}

fn read_lighting(agent: &mut Agent, id: &str) -> Option<Lighting> {
    let state = agent.get(&format!("/lighting/{id}/state"));
    let brightness = agent.get(&format!("/lighting/{id}/brightness")).or_else(|| agent.get(&format!("/lighting/{id}/firmware/brightness")));
    let on = state.as_ref().and_then(|payload| bool_at(payload, &["on", "enabled", "masterSwitch"]));
    let level = brightness.as_ref().and_then(|payload| number_at(payload, &["brightness", "value", "percent"]));
    let level = level.map(|n| if n <= 1.0 { (n * 100.0).round() } else { n.round() } as u32);
    if on.is_none() && level.is_none() {
        return None;
    }
    Some(Lighting { on, brightness: level })
}

struct Channel {
    percent: u32,
    muted: bool,
}

fn channel(payload: &Value, names: &[&str]) -> Option<Channel> {
    if let Some((name, percent, muted)) = parse_channel(payload) {
        if name.is_empty() || names.iter().any(|wanted| name.eq_ignore_ascii_case(wanted)) {
            return Some(Channel { percent, muted });
        }
    }
    for item in payload.get("channels").and_then(|v| v.as_array()).into_iter().flatten() {
        if let Some((name, percent, muted)) = parse_channel(item) {
            if names.iter().any(|wanted| name.eq_ignore_ascii_case(wanted)) {
                return Some(Channel { percent, muted });
            }
        }
    }
    None
}

fn parse_channel(payload: &Value) -> Option<(String, u32, bool)> {
    let gain = payload.get("gain").and_then(|n| n.as_f64())?;
    let percent = if gain <= 1.0 { (gain * 100.0).round() } else { gain.round() } as u32;
    let name = payload.get("channel").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let muted = payload.get("muted").and_then(|v| v.as_bool()).unwrap_or(false);
    Some((name, percent.min(100), muted))
}

pub fn apply(app: &AppHandle, change: DeviceApply) -> Result<DevicesSnapshot, String> {
    let mut agent = Agent::connect()?;
    let id = change.id.trim();
    if id.is_empty() {
        return Err("Choose a device first.".into());
    }
    if change.active_dpi.is_some() || change.dpi_levels.is_some() {
        set_dpi(&mut agent, id, change.active_dpi, change.dpi_levels.as_deref())?;
    }
    if let Some(rate) = change.report_rate {
        set_report_rate(&mut agent, id, rate)?;
    }
    if change.volume.is_some() || change.muted.is_some() {
        set_channel(&mut agent, id, "OUTPUT", "/volume", change.volume, change.muted)?;
    }
    if change.sidetone.is_some() || change.sidetone_muted.is_some() {
        set_channel(&mut agent, id, "SIDETONE", "/sidetone", change.sidetone, change.sidetone_muted)?;
    }
    if change.lighting_on.is_some() {
        let path = format!("/lighting/{id}/state");
        if let Some(mut payload) = agent.get(&path) {
            if set_bool(&mut payload, &["on", "enabled", "masterSwitch"], change.lighting_on.unwrap()) {
                agent.call("SET", &path, Some(payload))?;
            }
        }
    }
    if let Some(brightness) = change.brightness {
        let path = format!("/lighting/{id}/brightness");
        let firmware = format!("/lighting/{id}/firmware/brightness");
        let path = if agent.get(&path).is_some() { path } else { firmware };
        if let Some(mut payload) = agent.get(&path) {
            let value = brightness.min(100);
            if set_number(&mut payload, &["brightness", "value", "percent"], value) {
                agent.call("SET", &path, Some(payload))?;
            }
        }
    }
    let snap = read();
    *SNAP.lock().unwrap() = Some(snap.clone());
    let _ = app.emit("devices", &snap);
    Ok(snap)
}

fn dpi_table(payload: &mut Value) -> Option<&mut Value> {
    if payload.get("dpiTable").is_some() {
        return payload.get_mut("dpiTable");
    }
    if payload.get("mouseSettings").and_then(|settings| settings.get("dpiTable")).is_some() {
        return payload.get_mut("mouseSettings").and_then(|settings| settings.get_mut("dpiTable"));
    }
    if payload.get("activeDpi").is_some() {
        return Some(payload);
    }
    None
}

fn set_dpi(agent: &mut Agent, id: &str, active: Option<u32>, levels: Option<&[u32]>) -> Result<(), String> {
    let path = format!("/mouse/{id}/info");
    let Some(mut payload) = agent.get(&path) else {
        return Err("This device doesn't expose a DPI table.".into());
    };
    let Some(table) = dpi_table(&mut payload) else {
        return Err("This device doesn't expose a DPI table.".into());
    };
    let current: Vec<u32> = table.get("levels").and_then(|v| v.as_array()).map(|levels| levels.iter().filter_map(|n| n.as_u64().map(|n| n as u32)).collect()).unwrap_or_default();
    let next_levels: Vec<u32> = levels.unwrap_or(&current).iter().copied().map(|n| n.clamp(50, 45_000)).collect();
    if next_levels.is_empty() {
        return Err("A mouse needs at least one DPI step.".into());
    }
    let active = active.unwrap_or_else(|| table.get("activeDpi").and_then(|n| n.as_u64()).map(|n| n as u32).unwrap_or(next_levels[0]));
    if !next_levels.contains(&active) {
        return Err("The active DPI has to be one of the steps.".into());
    }
    table["levels"] = json!(next_levels);
    table["activeDpi"] = json!(active);
    if table.get("defaultDpi").and_then(|n| n.as_u64()).is_none_or(|n| n == 0) {
        table["defaultDpi"] = json!(active);
    }
    agent.call("SET", &path, Some(payload))?;
    Ok(())
}

fn set_report_rate(agent: &mut Agent, id: &str, rate: u32) -> Result<(), String> {
    let list_path = format!("/report_rate/{id}/report_rate_list");
    let wireless = format!("/wireless/{id}/report_rate");
    let allowed = agent.get(&list_path).or_else(|| agent.get(&wireless)).map(|payload| number_list(&payload)).unwrap_or_default();
    if !allowed.is_empty() && !allowed.contains(&rate) {
        return Err("That report rate isn't one this device offers.".into());
    }
    let mouse_path = format!("/mouse/{id}/info");
    if let Some(mut payload) = agent.get(&mouse_path) {
        let pointer = if payload.pointer("/reportRate/value").is_some() {
            Some("/reportRate/value")
        } else if payload.pointer("/mouseSettings/reportRate/value").is_some() {
            Some("/mouseSettings/reportRate/value")
        } else {
            None
        };
        if let Some(pointer) = pointer {
            if let Some(slot) = payload.pointer_mut(pointer) {
                *slot = json!(rate);
            }
            agent.call("SET", &mouse_path, Some(payload))?;
            return Ok(());
        }
    }
    if let Some(mut payload) = agent.get(&wireless) {
        if set_number(&mut payload, &["value", "reportRate"], rate) {
            agent.call("SET", &wireless, Some(payload))?;
            return Ok(());
        }
    }
    Err("This device doesn't expose a report rate.".into())
}

fn set_channel(agent: &mut Agent, id: &str, channel_name: &str, suffix: &str, percent: Option<u32>, muted: Option<bool>) -> Result<(), String> {
    let path = format!("/audio/{id}{suffix}");
    let current = agent.get(&path);
    let steps = current.as_ref().and_then(|payload| payload.get("steps")).cloned();
    let gain = percent.map(|p| p.min(100) as f64 / 100.0).or_else(|| current.as_ref().and_then(|p| p.get("gain").and_then(|n| n.as_f64())));
    let Some(gain) = gain else { return Err(format!("This device doesn't expose {channel_name} level.")) };
    let muted = muted.unwrap_or_else(|| current.as_ref().and_then(|p| p.get("muted").and_then(|v| v.as_bool())).unwrap_or(false));
    let mut payload = json!({ "channel": channel_name, "gain": gain, "muted": muted });
    if let Some(steps) = steps {
        payload["steps"] = steps;
    }
    agent.call("SET", &path, Some(payload))?;
    Ok(())
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            let snap = read();
            *SNAP.lock().unwrap() = Some(snap.clone());
            let _ = app.emit("devices", &snap);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn parse_known(item: &Value) -> Option<KnownDevice> {
    let model = text(item, "baseModelId").filter(|m| !m.is_empty()).or_else(|| text(item, "modelId")).filter(|m| !m.is_empty())?;
    let kind = text(item, "type").unwrap_or_else(|| "DEVICE".into());
    let last_seen = item.get("timestamp").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
    Some(KnownDevice { name: pretty_model(&model, &kind), model, kind: pretty_kind(&kind), last_seen })
}

fn pretty_kind(kind: &str) -> String {
    let kind = kind.trim_start_matches("INVALID_");
    if kind.is_empty() || kind == "INVALID_TYPE" {
        return "Device".into();
    }
    let mut chars = kind.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_ascii_lowercase(),
        None => "Device".into(),
    }
}

fn pretty_model(model: &str, kind: &str) -> String {
    let name = model.trim();
    if name.is_empty() {
        return pretty_kind(kind);
    }
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if i == 0 || name.chars().nth(i - 1).is_some_and(|prev| prev.is_ascii_digit() != ch.is_ascii_digit()) {
            out.extend(ch.to_uppercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn kind_is(kind: &str, names: &[&str]) -> bool {
    names.iter().any(|name| kind.eq_ignore_ascii_case(name))
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn array_at(value: &Value, keys: &[&str]) -> Vec<Value> {
    for key in keys {
        if let Some(items) = value.get(*key).and_then(|v| v.as_array()) {
            return items.clone();
        }
    }
    Vec::new()
}

fn number_at(value: &Value, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(number) = value.get(*key).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))) {
            return Some(number);
        }
    }
    value.as_object().and_then(|obj| {
        obj.values().find_map(|child| if child.is_object() { number_at(child, keys) } else { None })
    })
}

fn bool_at(value: &Value, keys: &[&str]) -> Option<bool> {
    for key in keys {
        if let Some(flag) = value.get(*key).and_then(|v| v.as_bool()) {
            return Some(flag);
        }
    }
    value.as_object().and_then(|obj| obj.values().find_map(|child| if child.is_object() { bool_at(child, keys) } else { None }))
}

fn number_list(value: &Value) -> Vec<u32> {
    let mut rates = Vec::new();
    collect_numbers(value, &mut rates, 0);
    rates.sort_unstable();
    rates.dedup();
    rates.retain(|n| (125..=8000).contains(n));
    rates
}

fn collect_numbers(value: &Value, out: &mut Vec<u32>, depth: u8) {
    if depth > 4 {
        return;
    }
    match value {
        Value::Array(items) => {
            for item in items {
                if let Some(n) = item.as_u64() {
                    out.push(n as u32);
                } else if let Some(n) = item.get("value").and_then(|v| v.as_u64()) {
                    out.push(n as u32);
                } else {
                    collect_numbers(item, out, depth + 1);
                }
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                if key == "@type" {
                    continue;
                }
                collect_numbers(child, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn set_bool(value: &mut Value, keys: &[&str], flag: bool) -> bool {
    if let Some(obj) = value.as_object_mut() {
        for key in keys {
            if obj.contains_key(*key) {
                obj.insert((*key).to_string(), json!(flag));
                return true;
            }
        }
        for child in obj.values_mut() {
            if child.is_object() && set_bool(child, keys, flag) {
                return true;
            }
        }
    }
    false
}

fn set_number(value: &mut Value, keys: &[&str], number: u32) -> bool {
    if let Some(obj) = value.as_object_mut() {
        for key in keys {
            if obj.contains_key(*key) {
                obj.insert((*key).to_string(), json!(number));
                return true;
            }
        }
        for child in obj.values_mut() {
            if child.is_object() && set_number(child, keys, number) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names() {
        assert_eq!(pretty_model("g433", "HEADSET"), "G433");
        assert_eq!(pretty_kind("HEADSET"), "Headset");
    }

    #[test]
    #[ignore]
    fn read_local_agent() {
        let snap = read();
        println!("{}", serde_json::to_string_pretty(&snap).unwrap());
        assert!(snap.agent);
    }
}
