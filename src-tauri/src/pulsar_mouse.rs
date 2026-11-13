//! Higher-level Pulsar mouse settings, built on the confirmed flash map in `pulsar`.
//!
//! Flash layout (verified against the X2 CrazyLight and Pulsar's own diff-writer):
//! - 0x00 polling-rate bitmask (1=125, 2=250, 4=500, 8=1000, 0x10=2000, 0x20=4000, 0x40=8000)
//! - 0x02 DPI stage count, 0x04 active stage (1-based)
//! - 0x0c + i*4 DPI stage i as [lo, lo, hi] with a trailing check byte at +3
//!
//! Each stored value is followed by its own check byte (`0x55 - sum`). Writes send `[value, check]`
//! exactly as Pulsar's firmware diff-writer does, and take effect immediately.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::pulsar::{self, Device, checksum};

// Flash addresses, each confirmed by watching Pulsar's own HID writes while a setting changed.
const ADDR_POLLING: u16 = 0x00;
const ADDR_DPI_COUNT: u16 = 0x02;
const ADDR_DPI_ACTIVE: u16 = 0x04;
const ADDR_LOD: u16 = 0x0a;
const ADDR_DPI_STAGES: u16 = 0x0c;
const ADDR_DEBOUNCE: u16 = 0xa9;
const ADDR_MOTION_SYNC: u16 = 0xab;
const ADDR_SLEEP: u16 = 0xad;
const ADDR_ANGLE_SNAP: u16 = 0xaf;
const ADDR_RIPPLE: u16 = 0xb1;
const ADDR_TURBO: u16 = 0xb5;
const ADDR_SLEEP_MIRROR: u16 = 0xb7;
const ADDR_LOW_POWER: u16 = 0xd7;
const MAX_STAGES: u8 = 8;

/// Low-power mode stores a battery percentage; Pulsar writes 100 for on and 0 for off.
const LOW_POWER_ON: u8 = 0x64;

/// Report rates the 8K hardware offers, as (bitmask, Hz).
const RATES: [(u8, u32); 7] = [(1, 125), (2, 250), (4, 500), (8, 1000), (0x10, 2000), (0x20, 4000), (0x40, 8000)];

/// Auto-sleep timer values Pulsar offers, in minutes.
const SLEEP_OPTIONS: [u8; 6] = [1, 3, 6, 30, 60, 180];
/// Lift-off distance levels the sensor accepts.
const LOD_OPTIONS: [u8; 3] = [1, 2, 3];

static SNAP: Mutex<Option<MouseSnapshot>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MouseSnapshot {
    pub present: bool,
    pub message: Option<String>,
    /// True when Pulsar's own app is running and contending for the mouse.
    pub vendor_app: bool,
    pub model: String,
    pub firmware: Option<String>,
    pub online: bool,
    pub battery: Option<u32>,
    pub charging: bool,
    pub dpi_stages: Vec<u32>,
    pub active_stage: u32,
    pub report_rate: Option<u32>,
    pub report_rates: Vec<u32>,
    pub motion_sync: bool,
    pub ripple: bool,
    pub angle_snap: bool,
    pub turbo: bool,
    pub low_power: bool,
    pub debounce_ms: u32,
    pub lod: u32,
    pub lod_options: Vec<u32>,
    pub sleep_minutes: u32,
    pub sleep_options: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseApply {
    pub dpi_stages: Option<Vec<u32>>,
    pub active_stage: Option<u32>,
    pub report_rate: Option<u32>,
    pub motion_sync: Option<bool>,
    pub ripple: Option<bool>,
    pub angle_snap: Option<bool>,
    pub turbo: Option<bool>,
    pub low_power: Option<bool>,
    pub debounce_ms: Option<u32>,
    pub lod: Option<u32>,
    pub sleep_minutes: Option<u32>,
    pub long_distance: Option<bool>,
}

pub fn latest() -> MouseSnapshot {
    SNAP.lock().unwrap().clone().unwrap_or(MouseSnapshot {
        present: false,
        message: Some("Looking for a Pulsar mouse…".into()),
        ..Default::default()
    })
}

/// Encode a DPI value into the 3 sensor bytes Pulsar stores (PULSAR XS1 family).
fn encode_dpi(dpi: u32) -> [u8; 3] {
    let dpi = dpi.clamp(50, 32000);
    if dpi < 10001 {
        let raw = (dpi / 10).saturating_sub(1);
        let lo = raw as u8;
        let hi = (((raw >> 8) & 3) as u8) * 0x11 << 2;
        [lo, lo, hi]
    } else {
        let base = if dpi > 30000 { dpi >> 1 } else { dpi };
        let raw = (base.saturating_sub(0x2742)) / 0x32;
        let lo = raw as u8;
        let mut hi = ((((raw >> 8) & 3) as u8) * 0x11 << 2) | 0x22;
        if dpi > 30000 {
            hi |= 0x33;
        }
        [lo, lo, hi]
    }
}

/// Decode the 3 stored sensor bytes back to a DPI value.
fn decode_dpi(bytes: [u8; 3]) -> u32 {
    let hi_marker = bytes[2];
    if hi_marker & 0x22 == 0 {
        let hi = ((hi_marker >> 2) & 3) as u32;
        let raw = ((hi << 8) | bytes[0] as u32) & 0x3ff;
        (raw + 1) * 10
    } else {
        let hi = ((hi_marker >> 2) & 3) as u32;
        let raw = (hi << 8) | bytes[0] as u32;
        let dpi = raw * 0x32 + 0x2742;
        if hi_marker & 0x11 != 0 { dpi * 2 } else { dpi }
    }
}

fn read_byte(device: &Device, addr: u16) -> Option<u8> {
    device.read_flash(addr, 1).ok().map(|d| d[0])
}

pub fn read() -> MouseSnapshot {
    let vendor_app = pulsar::vendor_app_running();
    let device = match pulsar::open() {
        Ok(device) => device,
        Err(message) => {
            return MouseSnapshot { present: false, message: Some(message), vendor_app, ..Default::default() };
        }
    };
    let online = device.online();
    let (battery, charging) = device.battery().map(|(p, c)| (Some(p as u32), c)).unwrap_or((None, false));
    let firmware = device.version().map(|v| format!("{}.{:02}", v >> 8, v & 0xff));

    let count = read_byte(&device, ADDR_DPI_COUNT).unwrap_or(0).min(MAX_STAGES);
    // The firmware stores the active stage 0-based; Helm's UI is 1-based.
    let active = read_byte(&device, ADDR_DPI_ACTIVE).unwrap_or(0) as u32 + 1;
    let mut dpi_stages = Vec::new();
    for i in 0..count {
        let addr = ADDR_DPI_STAGES + i as u16 * 4;
        if let Ok(window) = device.read_flash(addr, 3) {
            dpi_stages.push(decode_dpi([window[0], window[1], window[2]]));
        }
    }
    let report_rate = read_byte(&device, ADDR_POLLING).and_then(bit_to_hz);

    MouseSnapshot {
        present: true,
        message: None,
        vendor_app,
        model: device.product.clone(),
        firmware,
        online,
        battery,
        charging,
        dpi_stages,
        active_stage: active,
        report_rate,
        report_rates: RATES.iter().map(|(_, hz)| *hz).collect(),
        motion_sync: read_byte(&device, ADDR_MOTION_SYNC).unwrap_or(0) != 0,
        ripple: read_byte(&device, ADDR_RIPPLE).unwrap_or(0) != 0,
        angle_snap: read_byte(&device, ADDR_ANGLE_SNAP).unwrap_or(0) != 0,
        turbo: read_byte(&device, ADDR_TURBO).unwrap_or(0) != 0,
        low_power: read_byte(&device, ADDR_LOW_POWER).unwrap_or(0) != 0,
        debounce_ms: read_byte(&device, ADDR_DEBOUNCE).unwrap_or(0) as u32,
        lod: read_byte(&device, ADDR_LOD).unwrap_or(1) as u32,
        lod_options: LOD_OPTIONS.iter().map(|v| *v as u32).collect(),
        sleep_minutes: read_byte(&device, ADDR_SLEEP).unwrap_or(0) as u32,
        sleep_options: SLEEP_OPTIONS.iter().map(|v| *v as u32).collect(),
    }
}

fn bit_to_hz(bit: u8) -> Option<u32> {
    RATES.iter().find(|(b, _)| *b == bit).map(|(_, hz)| *hz)
}

fn hz_to_bit(hz: u32) -> Option<u8> {
    RATES.iter().find(|(_, h)| *h == hz).map(|(b, _)| *b)
}

/// Write one logical value as `[value, check]` at `addr`, matching Pulsar's diff-writer.
fn write_value(device: &Device, addr: u16, value: u8) -> Result<(), String> {
    device.write_flash(addr, &[value, checksum(&[value])])
}

pub fn apply(app: &AppHandle, change: MouseApply) -> Result<MouseSnapshot, String> {
    let device = pulsar::open()?;
    if let Some(stages) = &change.dpi_stages {
        if stages.is_empty() || stages.len() > MAX_STAGES as usize {
            return Err("A Pulsar mouse supports 1 to 8 DPI stages.".into());
        }
        write_value(&device, ADDR_DPI_COUNT, stages.len() as u8)?;
        for (i, dpi) in stages.iter().enumerate() {
            let addr = ADDR_DPI_STAGES + i as u16 * 4;
            let bytes = encode_dpi(*dpi);
            let check = checksum(&bytes);
            device.write_flash(addr, &[bytes[0], bytes[1], bytes[2], check])?;
        }
    }
    if let Some(active) = change.active_stage {
        let count = change.dpi_stages.as_ref().map(|s| s.len() as u32).unwrap_or_else(|| read_byte(&device, ADDR_DPI_COUNT).unwrap_or(1) as u32);
        if active < 1 || active > count {
            return Err("The active DPI stage has to be one of the configured stages.".into());
        }
        // Convert the 1-based UI stage to the firmware's 0-based index.
        write_value(&device, ADDR_DPI_ACTIVE, (active - 1) as u8)?;
    }
    if let Some(hz) = change.report_rate {
        let bit = hz_to_bit(hz).ok_or("That report rate isn't one the mouse offers.")?;
        write_value(&device, ADDR_POLLING, bit)?;
    }
    if let Some(on) = change.motion_sync {
        write_value(&device, ADDR_MOTION_SYNC, on as u8)?;
    }
    if let Some(on) = change.ripple {
        write_value(&device, ADDR_RIPPLE, on as u8)?;
    }
    if let Some(on) = change.angle_snap {
        write_value(&device, ADDR_ANGLE_SNAP, on as u8)?;
    }
    if let Some(on) = change.turbo {
        write_value(&device, ADDR_TURBO, on as u8)?;
    }
    if let Some(on) = change.low_power {
        write_value(&device, ADDR_LOW_POWER, if on { LOW_POWER_ON } else { 0 })?;
    }
    if let Some(ms) = change.debounce_ms {
        write_value(&device, ADDR_DEBOUNCE, ms.min(255) as u8)?;
    }
    if let Some(lod) = change.lod {
        if !LOD_OPTIONS.contains(&(lod as u8)) {
            return Err("That lift-off distance isn't one the sensor accepts.".into());
        }
        write_value(&device, ADDR_LOD, lod as u8)?;
    }
    if let Some(minutes) = change.sleep_minutes {
        if !SLEEP_OPTIONS.contains(&(minutes as u8)) {
            return Err("That sleep timer isn't one the mouse offers.".into());
        }
        // Pulsar mirrors the sleep timer to both bytes.
        write_value(&device, ADDR_SLEEP, minutes as u8)?;
        write_value(&device, ADDR_SLEEP_MIRROR, minutes as u8)?;
    }
    if let Some(on) = change.long_distance {
        device.set_long_range(on)?;
    }
    std::thread::sleep(Duration::from_millis(60));
    let snap = read();
    *SNAP.lock().unwrap() = Some(snap.clone());
    let _ = app.emit("pulsar", &snap);
    Ok(snap)
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            let snap = read();
            *SNAP.lock().unwrap() = Some(snap.clone());
            let _ = app.emit("pulsar", &snap);
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_round_trips() {
        for dpi in [400u32, 800, 1200, 1600, 3200, 6400, 12800, 20000, 26000] {
            let decoded = decode_dpi(encode_dpi(dpi));
            // Encoding snaps to the sensor's 10- or 50-DPI grid, so allow one step of error.
            assert!((decoded as i64 - dpi as i64).abs() <= 100, "{dpi} -> {decoded}");
        }
    }
}
