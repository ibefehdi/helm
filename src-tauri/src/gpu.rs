//! NVIDIA graphics tuning through the driver's NVML library (the supported interface behind
//! Afterburner's NVAPI path), plus frametime and an overlay slot in RivaTuner Statistics Server.
//! Fan curves are applied by Helm from GPU temperature. The kernel helper Afterburner ships is not used.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::model::GpuControl;
use crate::state::AppState;

const CLOCK_OFFSET_VERSION: u32 = (1 << 24) | 24;
const CLOCK_GRAPHICS: u32 = 0;
const CLOCK_MEM: u32 = 2;
const TEMP_GPU: u32 = 0;
const TEMP_SLOWDOWN: u32 = 1;
const TEMP_GPU_MAX: u32 = 3;
const TEMP_ACOUSTIC_MIN: u32 = 4;
const TEMP_ACOUSTIC_MAX: u32 = 6;
const FAN_AUTO: u32 = 0;
const FAN_MANUAL: u32 = 1;
const CONFIRM_SECS: u64 = 20;
const OSD_OWNER: &str = "Helm";

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GpuDevice {
    pub index: u32,
    pub name: String,
    pub temperature: Option<u32>,
    pub fan_percent: Option<u32>,
    pub fan_rpm: Option<u32>,
    pub fan_count: u32,
    pub fan_auto: bool,
    pub graphics_clock: Option<u32>,
    pub memory_clock: Option<u32>,
    pub utilization: Option<u32>,
    pub memory_controller: Option<u32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub power_default_w: Option<f64>,
    pub power_min_w: Option<f64>,
    pub power_max_w: Option<f64>,
    pub temp_limit: Option<u32>,
    pub temp_limit_min: Option<u32>,
    pub temp_limit_max: Option<u32>,
    pub temp_slowdown: Option<u32>,
    pub core_offset: Option<i32>,
    pub core_offset_min: Option<i32>,
    pub core_offset_max: Option<i32>,
    pub memory_offset: Option<i32>,
    pub memory_offset_min: Option<i32>,
    pub memory_offset_max: Option<i32>,
    pub performance_state: Option<u32>,
    pub limits: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FrameStats {
    pub rtss: bool,
    pub fps: Option<f64>,
    pub frame_ms: Option<f64>,
    pub app: Option<String>,
    pub one_percent_low: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuPoint {
    pub t: i64,
    pub temperature: Option<u32>,
    pub power_w: Option<f64>,
    pub utilization: Option<u32>,
    pub graphics_clock: Option<u32>,
    pub fps: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSnapshot {
    pub available: bool,
    pub message: Option<String>,
    pub devices: Vec<GpuDevice>,
    pub frames: FrameStats,
    pub pending_confirm: bool,
    pub history: Vec<GpuPoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuApply {
    pub index: u32,
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

#[repr(C)]
struct ClockOffset {
    version: u32,
    kind: u32,
    pstate: u32,
    offset: i32,
    min: i32,
    max: i32,
}

#[repr(C)]
struct Util {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
struct MemInfo {
    total: u64,
    free: u64,
    used: u64,
}

#[derive(Clone)]
struct Pending {
    token: u64,
    index: u32,
    core: i32,
    memory: i32,
}

static LIB: Mutex<Option<Nvml>> = Mutex::new(None);
static SNAP: Mutex<Option<GpuSnapshot>> = Mutex::new(None);
static HISTORY: Mutex<Vec<GpuPoint>> = Mutex::new(Vec::new());
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static TOKEN: AtomicU64 = AtomicU64::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
static FAN_SET: Mutex<Option<u32>> = Mutex::new(None);

struct Nvml {
    module: *mut c_void,
    devices: Vec<Device>,
    get_temp: Fn2,
    get_threshold: Fn2,
    set_threshold: Set2,
    get_clock: Fn2,
    get_util: UtilFn,
    get_mem: MemFn,
    get_power: U32Fn,
    get_power_limit: U32Fn,
    get_power_default: U32Fn,
    get_power_bounds: BoundsFn,
    set_power: SetMw,
    get_pstate: U32Fn,
    get_offsets: OffsetFn,
    set_offsets: OffsetFn,
    get_fans: U32Fn,
    get_fan_pct: FanFn,
    get_fan_rpm: FanFn,
    set_fan_pct: SetFan,
    get_fan_policy: FanFn,
    set_fan_policy: SetFan,
    set_fan_default: FanSet,
    get_throttle: ThrottleFn,
}

type Device = *mut c_void;
type Fn2 = Option<unsafe extern "C" fn(Device, u32, *mut u32) -> i32>;
type Set2 = Option<unsafe extern "C" fn(Device, u32, u32) -> i32>;
type UtilFn = Option<unsafe extern "C" fn(Device, *mut Util) -> i32>;
type MemFn = Option<unsafe extern "C" fn(Device, *mut MemInfo) -> i32>;
type U32Fn = Option<unsafe extern "C" fn(Device, *mut u32) -> i32>;
type BoundsFn = Option<unsafe extern "C" fn(Device, *mut u32, *mut u32) -> i32>;
type SetMw = Option<unsafe extern "C" fn(Device, u32) -> i32>;
type OffsetFn = Option<unsafe extern "C" fn(Device, *mut ClockOffset) -> i32>;
type FanFn = Option<unsafe extern "C" fn(Device, u32, *mut u32) -> i32>;
type SetFan = Option<unsafe extern "C" fn(Device, u32, u32) -> i32>;
type FanSet = Option<unsafe extern "C" fn(Device, u32) -> i32>;
type ThrottleFn = Option<unsafe extern "C" fn(Device, *mut u64) -> i32>;

unsafe impl Send for Nvml {}

fn proc<T>(module: *mut c_void, name: &str) -> Option<T> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    let c = std::ffi::CString::new(name).ok()?;
    let p = unsafe { GetProcAddress(module, c.as_ptr().cast()) }?;
    Some(unsafe { std::mem::transmute_copy(&p) })
}

fn load() -> Result<(), String> {
    if LIB.lock().unwrap().is_some() {
        return Ok(());
    }
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
    let wide: Vec<u16> = "nvml.dll".encode_utf16().chain(Some(0)).collect();
    let module = unsafe { LoadLibraryW(wide.as_ptr()) };
    if module.is_null() {
        return Err("The NVIDIA management library (nvml.dll) isn't installed. Graphics tuning needs an NVIDIA driver.".into());
    }
    type Init = unsafe extern "C" fn() -> i32;
    type Count = unsafe extern "C" fn(*mut u32) -> i32;
    type Handle = unsafe extern "C" fn(u32, *mut Device) -> i32;
    type Name = unsafe extern "C" fn(Device, *mut i8, u32) -> i32;
    let init: Init = proc(module, "nvmlInit_v2").ok_or("nvmlInit_v2 is missing")?;
    let rc = unsafe { init() };
    if rc != 0 && rc != 5 {
        return Err(format!("NVIDIA driver refused to initialize (NVML {rc})."));
    }
    let count_fn: Count = proc(module, "nvmlDeviceGetCount_v2").ok_or("nvmlDeviceGetCount_v2 is missing")?;
    let handle_fn: Handle = proc(module, "nvmlDeviceGetHandleByIndex_v2").ok_or("nvmlDeviceGetHandleByIndex_v2 is missing")?;
    let name_fn: Name = proc(module, "nvmlDeviceGetName").ok_or("nvmlDeviceGetName is missing")?;
    let mut n = 0u32;
    let rc = unsafe { count_fn(&mut n) };
    if rc != 0 {
        return Err(format!("Couldn't enumerate NVIDIA GPUs (NVML {rc})."));
    }
    let mut devices = Vec::new();
    for i in 0..n {
        let mut h: Device = std::ptr::null_mut();
        if unsafe { handle_fn(i, &mut h) } == 0 && !h.is_null() {
            devices.push(h);
            let _ = name_fn;
        }
    }
    if devices.is_empty() {
        return Err("No NVIDIA GPU was found.".into());
    }
    *LIB.lock().unwrap() = Some(Nvml {
        module,
        devices,
        get_temp: proc(module, "nvmlDeviceGetTemperature"),
        get_threshold: proc(module, "nvmlDeviceGetTemperatureThreshold"),
        set_threshold: proc(module, "nvmlDeviceSetTemperatureThreshold"),
        get_clock: proc(module, "nvmlDeviceGetClockInfo"),
        get_util: proc(module, "nvmlDeviceGetUtilizationRates"),
        get_mem: proc(module, "nvmlDeviceGetMemoryInfo"),
        get_power: proc(module, "nvmlDeviceGetPowerUsage"),
        get_power_limit: proc(module, "nvmlDeviceGetPowerManagementLimit"),
        get_power_default: proc(module, "nvmlDeviceGetPowerManagementDefaultLimit"),
        get_power_bounds: proc(module, "nvmlDeviceGetPowerManagementLimitConstraints"),
        set_power: proc(module, "nvmlDeviceSetPowerManagementLimit"),
        get_pstate: proc(module, "nvmlDeviceGetPerformanceState"),
        get_offsets: proc(module, "nvmlDeviceGetClockOffsets"),
        set_offsets: proc(module, "nvmlDeviceSetClockOffsets"),
        get_fans: proc(module, "nvmlDeviceGetNumFans"),
        get_fan_pct: proc(module, "nvmlDeviceGetFanSpeed_v2"),
        get_fan_rpm: proc(module, "nvmlDeviceGetFanSpeedRPM"),
        set_fan_pct: proc(module, "nvmlDeviceSetFanSpeed_v2"),
        get_fan_policy: proc(module, "nvmlDeviceGetFanControlPolicy_v2"),
        set_fan_policy: proc(module, "nvmlDeviceSetFanControlPolicy"),
        set_fan_default: proc(module, "nvmlDeviceSetDefaultFanSpeed_v2"),
        get_throttle: proc(module, "nvmlDeviceGetCurrentClocksThrottleReasons"),
    });
    let _ = (module, name_fn);
    Ok(())
}

fn with_nvml<T>(f: impl FnOnce(&Nvml) -> T) -> Result<T, String> {
    load()?;
    let guard = LIB.lock().unwrap();
    Ok(f(guard.as_ref().unwrap()))
}

fn opt_u32(f: Fn2, d: Device, a: u32) -> Option<u32> {
    let f = f?;
    let mut v = 0u32;
    (unsafe { f(d, a, &mut v) } == 0).then_some(v)
}

fn watts(mw: Option<u32>) -> Option<f64> {
    mw.map(|v| (v as f64 / 1000.0 * 10.0).round() / 10.0)
}

fn clock_offset(get: OffsetFn, d: Device, kind: u32) -> Option<(i32, i32, i32)> {
    let get = get?;
    let mut info = ClockOffset { version: CLOCK_OFFSET_VERSION, kind, pstate: 0, offset: 0, min: 0, max: 0 };
    (unsafe { get(d, &mut info) } == 0).then_some((info.offset, info.min, info.max))
}

fn throttle_labels(bits: u64) -> Vec<String> {
    let flags = [
        (0x4u64, "Power limit"),
        (0x8, "Hardware slowdown"),
        (0x20, "Thermal limit"),
        (0x40, "Hardware thermal limit"),
        (0x80, "Power brake"),
    ];
    flags.into_iter().filter(|(b, _)| bits & b != 0).map(|(_, s)| s.to_string()).collect()
}

fn read_device(nv: &Nvml, index: u32, device: Device, module: *mut c_void) -> GpuDevice {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    let mut gpu = GpuDevice { index, ..Default::default() };
    let cname = std::ffi::CString::new("nvmlDeviceGetName").unwrap();
    if let Some(p) = unsafe { GetProcAddress(module, cname.as_ptr().cast()) } {
        type Name = unsafe extern "C" fn(Device, *mut i8, u32) -> i32;
        let name: Name = unsafe { std::mem::transmute(p) };
        let mut buf = [0i8; 96];
        if unsafe { name(device, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
            let bytes = buf.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect::<Vec<_>>();
            gpu.name = String::from_utf8_lossy(&bytes).into_owned();
        }
    }
    gpu.temperature = opt_u32(nv.get_temp, device, TEMP_GPU);
    gpu.temp_slowdown = opt_u32(nv.get_threshold, device, TEMP_SLOWDOWN);
    gpu.temp_limit = opt_u32(nv.get_threshold, device, TEMP_GPU_MAX);
    gpu.temp_limit_min = opt_u32(nv.get_threshold, device, TEMP_ACOUSTIC_MIN);
    gpu.temp_limit_max = opt_u32(nv.get_threshold, device, TEMP_ACOUSTIC_MAX);
    gpu.graphics_clock = opt_u32(nv.get_clock, device, CLOCK_GRAPHICS);
    gpu.memory_clock = opt_u32(nv.get_clock, device, CLOCK_MEM);
    if let Some(f) = nv.get_util {
        let mut u = Util { gpu: 0, memory: 0 };
        if unsafe { f(device, &mut u) } == 0 {
            gpu.utilization = Some(u.gpu);
            gpu.memory_controller = Some(u.memory);
        }
    }
    if let Some(f) = nv.get_mem {
        let mut m = MemInfo { total: 0, free: 0, used: 0 };
        if unsafe { f(device, &mut m) } == 0 {
            gpu.memory_used = Some(m.used);
            gpu.memory_total = Some(m.total);
        }
    }
    let mw = |f: U32Fn| {
        let f = f?;
        let mut v = 0u32;
        (unsafe { f(device, &mut v) } == 0).then_some(v)
    };
    gpu.power_w = watts(mw(nv.get_power));
    gpu.power_limit_w = watts(mw(nv.get_power_limit));
    gpu.power_default_w = watts(mw(nv.get_power_default));
    if let Some(f) = nv.get_power_bounds {
        let (mut lo, mut hi) = (0u32, 0u32);
        if unsafe { f(device, &mut lo, &mut hi) } == 0 {
            gpu.power_min_w = watts(Some(lo));
            gpu.power_max_w = watts(Some(hi));
        }
    }
    gpu.performance_state = mw(nv.get_pstate);
    if let Some((cur, min, max)) = clock_offset(nv.get_offsets, device, CLOCK_GRAPHICS) {
        gpu.core_offset = Some(cur);
        gpu.core_offset_min = Some(min);
        gpu.core_offset_max = Some(max);
    }
    if let Some((cur, min, max)) = clock_offset(nv.get_offsets, device, CLOCK_MEM) {
        gpu.memory_offset = Some(cur);
        gpu.memory_offset_min = Some(min);
        gpu.memory_offset_max = Some(max);
    }
    gpu.fan_count = mw(nv.get_fans).unwrap_or(0).min(8);
    if gpu.fan_count == 0 {
        gpu.fan_count = 1;
    }
    gpu.fan_percent = nv.get_fan_pct.and_then(|f| {
        let mut v = 0u32;
        (unsafe { f(device, 0, &mut v) } == 0).then_some(v)
    });
    gpu.fan_rpm = nv.get_fan_rpm.and_then(|f| {
        let mut v = 0u32;
        (unsafe { f(device, 0, &mut v) } == 0 && v > 0).then_some(v)
    });
    gpu.fan_auto = nv.get_fan_policy.map(|f| {
        let mut v = FAN_AUTO;
        unsafe { f(device, 0, &mut v) };
        v == FAN_AUTO
    }).unwrap_or(true);
    if let Some(f) = nv.get_throttle {
        let mut bits = 0u64;
        if unsafe { f(device, &mut bits) } == 0 {
            gpu.limits = throttle_labels(bits);
        }
    }
    let _ = module;
    gpu
}

pub fn read() -> Result<Vec<GpuDevice>, String> {
    with_nvml(|nv| {
        let module = nv.module;
        nv.devices.iter().enumerate().map(|(i, d)| read_device(nv, i as u32, *d, module)).collect()
    })
}

fn set_offset(nv: &Nvml, device: Device, kind: u32, offset: i32, min: i32, max: i32) -> Result<(), String> {
    let set = nv.set_offsets.ok_or("This driver can't set clock offsets.")?;
    if offset < min || offset > max {
        return Err(format!("Offset {offset} MHz is outside this card's range ({min} to {max})."));
    }
    let mut info = ClockOffset { version: CLOCK_OFFSET_VERSION, kind, pstate: 0, offset, min, max };
    let rc = unsafe { set(device, &mut info) };
    if rc != 0 {
        return Err(nvml_message(rc, "clock offset"));
    }
    Ok(())
}

fn nvml_message(rc: i32, what: &str) -> String {
    match rc {
        3 => format!("This GPU doesn't support changing the {what}."),
        4 => format!("The driver refused the {what}. Helm needs to be running as administrator."),
        _ => format!("The driver rejected the {what} (NVML {rc})."),
    }
}

fn device<'a>(nv: &'a Nvml, index: u32) -> Result<Device, String> {
    nv.devices.get(index as usize).copied().ok_or_else(|| format!("No NVIDIA GPU at index {index}."))
}

fn apply_fan(nv: &Nvml, device: Device, mode: &str, percent: u32, temp: Option<u32>, slowdown: Option<u32>, curve: &[[f64; 2]]) -> Result<(), String> {
    let fans = {
        let mut n = 0u32;
        if let Some(f) = nv.get_fans {
            if unsafe { f(device, &mut n) } != 0 || n == 0 {
                n = 1;
            }
        } else {
            n = 1;
        }
        n.min(4)
    };
    let hot = temp.is_some_and(|t| t >= slowdown.unwrap_or(90).saturating_sub(2) || t >= 90);
    let (policy, speed) = if hot {
        (FAN_MANUAL, 100)
    } else {
        match mode {
            "manual" => (FAN_MANUAL, percent.min(100)),
            "curve" => (FAN_MANUAL, curve_percent(curve, temp.unwrap_or(50) as f64)),
            _ => (FAN_AUTO, 0),
        }
    };
    if policy == FAN_AUTO {
        if let Some(f) = nv.set_fan_policy {
            for i in 0..fans {
                let rc = unsafe { f(device, i, FAN_AUTO) };
                if rc != 0 && rc != 3 {
                    return Err(nvml_message(rc, "fan"));
                }
            }
        }
        *FAN_SET.lock().unwrap() = None;
        return Ok(());
    }
    let last = *FAN_SET.lock().unwrap();
    if last == Some(speed) && !hot {
        return Ok(());
    }
    let set_policy = nv.set_fan_policy.ok_or("This driver doesn't expose fan control.")?;
    let set_speed = nv.set_fan_pct.ok_or("This driver doesn't expose fan control.")?;
    for i in 0..fans {
        let rc = unsafe { set_policy(device, i, FAN_MANUAL) };
        if rc != 0 {
            return Err(nvml_message(rc, "fan"));
        }
        let rc = unsafe { set_speed(device, i, speed) };
        if rc != 0 {
            return Err(nvml_message(rc, "fan speed"));
        }
    }
    *FAN_SET.lock().unwrap() = Some(speed);
    let _ = nv.set_fan_default;
    Ok(())
}

fn curve_percent(curve: &[[f64; 2]], temp: f64) -> u32 {
    if curve.len() < 2 {
        return 50;
    }
    if temp <= curve[0][0] {
        return curve[0][1].clamp(0.0, 100.0).round() as u32;
    }
    for w in curve.windows(2) {
        if temp <= w[1][0] && w[1][0] > w[0][0] {
            let t = (temp - w[0][0]) / (w[1][0] - w[0][0]);
            return (w[0][1] + t * (w[1][1] - w[0][1])).clamp(0.0, 100.0).round() as u32;
        }
    }
    curve.last().map(|p| p[1].clamp(0.0, 100.0).round() as u32).unwrap_or(100)
}

pub fn release_fans() {
    let Ok(nv_guard) = LIB.lock() else { return };
    let Some(nv) = nv_guard.as_ref() else { return };
    for device in &nv.devices {
        let _ = apply_fan(nv, *device, "auto", 0, None, None, &[]);
    }
}

#[derive(Serialize, Deserialize)]
struct PendingFile {
    index: u32,
    core: i32,
    memory: i32,
}

fn pending_path(app: &AppHandle) -> std::path::PathBuf {
    app.state::<AppState>().data_dir.join("gpu-pending.json")
}

fn arm_pending(app: &AppHandle, index: u32, core: i32, memory: i32) -> u64 {
    let token = TOKEN.fetch_add(1, Ordering::SeqCst) + 1;
    *PENDING.lock().unwrap() = Some(Pending { token, index, core, memory });
    let path = pending_path(app);
    let _ = std::fs::write(&path, serde_json::to_vec(&PendingFile { index, core, memory }).unwrap_or_default());
    token
}

fn clear_pending(app: &AppHandle) {
    PENDING.lock().unwrap().take();
    let _ = std::fs::remove_file(pending_path(app));
}

/// A clock change that was never confirmed (Helm closed or froze) is undone on the next launch.
fn revert_abandoned(app: &AppHandle) {
    let path = pending_path(app);
    let Ok(bytes) = std::fs::read(&path) else { return };
    let Ok(saved) = serde_json::from_slice::<PendingFile>(&bytes) else { return };
    if let Ok(nv) = with_nvml(|nv| {
        let device = device(nv, saved.index)?;
        let core = clock_offset(nv.get_offsets, device, CLOCK_GRAPHICS);
        let mem = clock_offset(nv.get_offsets, device, CLOCK_MEM);
        if let Some((_, min, max)) = core {
            set_offset(nv, device, CLOCK_GRAPHICS, saved.core.clamp(min, max), min, max)?;
        }
        if let Some((_, min, max)) = mem {
            set_offset(nv, device, CLOCK_MEM, saved.memory.clamp(min, max), min, max)?;
        }
        Ok::<(), String>(())
    }) {
        let _ = nv;
    }
    let _ = std::fs::remove_file(path);
    let _ = app.emit("toast", json!({ "kind": "warn", "message": "Reverted graphics clocks that were never confirmed." }));
}

pub fn apply(app: &AppHandle, change: GpuApply) -> Result<GpuSnapshot, String> {
    let clocks_changed = with_nvml(|nv| {
        let module = nv.module;
        let device = device(nv, change.index)?;
        let before_core = clock_offset(nv.get_offsets, device, CLOCK_GRAPHICS);
        let before_mem = clock_offset(nv.get_offsets, device, CLOCK_MEM);
        if let Some((cur, min, max)) = before_core {
            if change.core_offset != cur {
                set_offset(nv, device, CLOCK_GRAPHICS, change.core_offset, min, max)?;
            }
        }
        if let Some((cur, min, max)) = before_mem {
            if change.memory_offset != cur {
                set_offset(nv, device, CLOCK_MEM, change.memory_offset, min, max)?;
            }
        }
        if let Some(watts) = change.power_limit_w {
            if let Some(set) = nv.set_power {
                let (mut lo, mut hi) = (0u32, 0u32);
                if let Some(bounds) = nv.get_power_bounds {
                    if unsafe { bounds(device, &mut lo, &mut hi) } != 0 {
                        lo = 0;
                        hi = u32::MAX;
                    }
                }
                let mw = (watts * 1000.0).round() as u32;
                if lo > 0 && (mw < lo || mw > hi) {
                    return Err(format!("Power limit must stay between {:.0} W and {:.0} W.", lo as f64 / 1000.0, hi as f64 / 1000.0));
                }
                let rc = unsafe { set(device, mw) };
                if rc != 0 {
                    return Err(nvml_message(rc, "power limit"));
                }
            }
        }
        if let Some(temp) = change.temp_limit_c {
            if let Some(set) = nv.set_threshold {
                let rc = unsafe { set(device, TEMP_GPU_MAX, temp) };
                if rc != 0 && rc != 3 {
                    return Err(nvml_message(rc, "temperature limit"));
                }
            }
        }
        let live = read_device(nv, change.index, device, module);
        apply_fan(nv, device, &change.fan_mode, change.fan_percent, live.temperature, live.temp_slowdown, &change.fan_curve)?;
        let core_moved = before_core.is_some_and(|(cur, _, _)| cur != change.core_offset);
        let mem_moved = before_mem.is_some_and(|(cur, _, _)| cur != change.memory_offset);
        let baseline = if PENDING.lock().unwrap().is_some() {
            None
        } else if core_moved || mem_moved {
            Some((before_core.map(|c| c.0).unwrap_or(0), before_mem.map(|c| c.0).unwrap_or(0)))
        } else {
            None
        };
        Ok::<_, String>(baseline)
    })??;

    {
        let state = app.state::<AppState>();
        let mut settings = state.settings.lock().unwrap();
        settings.gpu.fan_mode = change.fan_mode;
        settings.gpu.fan_percent = change.fan_percent.min(100);
        settings.gpu.fan_curve = change.fan_curve;
        settings.gpu.power_limit_w = change.power_limit_w;
        settings.gpu.temp_limit_c = change.temp_limit_c;
        settings.gpu.apply_on_start = change.apply_on_start;
        settings.gpu.osd = change.osd;
        // Offsets stay unconfirmed while a revert timer is armed, including a second change made during it.
        if clocks_changed.is_none() && PENDING.lock().unwrap().is_none() {
            settings.gpu.core_offset = change.core_offset;
            settings.gpu.memory_offset = change.memory_offset;
        }
        drop(settings);
        state.persist_settings()?;
        let _ = app.emit("settings", state.settings.lock().unwrap().clone());
    }

    if let Some((core, memory)) = clocks_changed {
        let token = arm_pending(app, change.index, core, memory);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(CONFIRM_SECS)).await;
            let due = {
                let mut pending = PENDING.lock().unwrap();
                if pending.as_ref().is_some_and(|p| p.token == token) {
                    pending.take()
                } else {
                    None
                }
            };
            if let Some(p) = due {
                let result = with_nvml(|nv| {
                    let device = device(nv, p.index)?;
                    if let Some((_, min, max)) = clock_offset(nv.get_offsets, device, CLOCK_GRAPHICS) {
                        set_offset(nv, device, CLOCK_GRAPHICS, p.core.clamp(min, max), min, max)?;
                    }
                    if let Some((_, min, max)) = clock_offset(nv.get_offsets, device, CLOCK_MEM) {
                        set_offset(nv, device, CLOCK_MEM, p.memory.clamp(min, max), min, max)?;
                    }
                    Ok::<(), String>(())
                });
                let _ = std::fs::remove_file(pending_path(&app));
                let _ = app.emit("gpu-reverted", json!({ "ok": result.and_then(|r| r).is_ok() }));
                let _ = publish(&app);
            }
        });
    }
    publish(app)
}

pub fn confirm(app: &AppHandle) -> Result<bool, String> {
    let Some(pending) = PENDING.lock().unwrap().clone() else { return Ok(false) };
    let devices = read()?;
    if let Some(gpu) = devices.iter().find(|d| d.index == pending.index) {
        let state = app.state::<AppState>();
        let mut settings = state.settings.lock().unwrap();
        settings.gpu.core_offset = gpu.core_offset.unwrap_or(0);
        settings.gpu.memory_offset = gpu.memory_offset.unwrap_or(0);
        drop(settings);
        let _ = state.persist_settings();
        let _ = app.emit("settings", state.settings.lock().unwrap().clone());
    }
    clear_pending(app);
    Ok(true)
}

pub fn restore_clocks(app: &AppHandle) -> Result<GpuSnapshot, String> {
    let pending = PENDING.lock().unwrap().clone();
    let state = app.state::<AppState>();
    let saved = state.settings.lock().unwrap().gpu.clone();
    let (index, core, memory) = pending
        .as_ref()
        .map(|p| (p.index, p.core, p.memory))
        .unwrap_or((0, saved.core_offset, saved.memory_offset));
    with_nvml(|nv| {
        let device = device(nv, index)?;
        if let Some((_, min, max)) = clock_offset(nv.get_offsets, device, CLOCK_GRAPHICS) {
            set_offset(nv, device, CLOCK_GRAPHICS, core.clamp(min, max), min, max)?;
        }
        if let Some((_, min, max)) = clock_offset(nv.get_offsets, device, CLOCK_MEM) {
            set_offset(nv, device, CLOCK_MEM, memory.clamp(min, max), min, max)?;
        }
        Ok::<(), String>(())
    })??;
    clear_pending(app);
    publish(app)
}

fn publish(app: &AppHandle) -> Result<GpuSnapshot, String> {
    let devices = read().unwrap_or_default();
    let frames = read_frames();
    let history = {
        let mut history = HISTORY.lock().unwrap();
        if let Some(gpu) = devices.first() {
            history.push(GpuPoint {
                t: chrono::Utc::now().timestamp_millis(),
                temperature: gpu.temperature,
                power_w: gpu.power_w,
                utilization: gpu.utilization,
                graphics_clock: gpu.graphics_clock,
                fps: frames.fps,
            });
            if history.len() > 180 {
                let keep = history.len() - 180;
                history.drain(0..keep);
            }
        }
        history.clone()
    };
    let snap = GpuSnapshot {
        available: !devices.is_empty(),
        message: devices.is_empty().then(|| "No NVIDIA GPU.".into()),
        pending_confirm: PENDING.lock().unwrap().is_some(),
        history,
        frames,
        devices,
    };
    *SNAP.lock().unwrap() = Some(snap.clone());
    let _ = app.emit("gpu", &snap);
    Ok(snap)
}

pub fn latest() -> GpuSnapshot {
    SNAP.lock().unwrap().clone().unwrap_or(GpuSnapshot {
        available: false,
        message: Some("Reading the GPU…".into()),
        devices: Vec::new(),
        frames: FrameStats::default(),
        pending_confirm: false,
        history: Vec::new(),
    })
}

pub fn spawn(app: AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        revert_abandoned(&app);
        let mut applied_start = false;
        loop {
            if !applied_start {
                applied_start = true;
                let gpu = app.state::<AppState>().settings.lock().unwrap().gpu.clone();
                if gpu.apply_on_start {
                    let _ = apply_saved(&app, &gpu);
                }
            }
            if let Ok(snap) = publish(&app) {
                let gpu = app.state::<AppState>().settings.lock().unwrap().gpu.clone();
                if gpu.fan_mode != "auto" {
                    if let Some(live) = snap.devices.first() {
                        let _ = with_nvml(|nv| {
                            if let Ok(device) = device(nv, live.index) {
                                let _ = apply_fan(nv, device, &gpu.fan_mode, gpu.fan_percent, live.temperature, live.temp_slowdown, &gpu.fan_curve);
                            }
                        });
                    }
                }
                if gpu.osd {
                    let _ = write_osd(&snap);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

fn apply_saved(app: &AppHandle, gpu: &GpuControl) -> Result<(), String> {
    let _ = app;
    with_nvml(|nv| {
        let device = device(nv, 0)?;
        if let Some((_, min, max)) = clock_offset(nv.get_offsets, device, CLOCK_GRAPHICS) {
            set_offset(nv, device, CLOCK_GRAPHICS, gpu.core_offset.clamp(min, max), min, max)?;
        }
        if let Some((_, min, max)) = clock_offset(nv.get_offsets, device, CLOCK_MEM) {
            set_offset(nv, device, CLOCK_MEM, gpu.memory_offset.clamp(min, max), min, max)?;
        }
        if let (Some(watts), Some(set)) = (gpu.power_limit_w, nv.set_power) {
            let rc = unsafe { set(device, (watts * 1000.0).round() as u32) };
            if rc != 0 {
                return Err(nvml_message(rc, "power limit"));
            }
        }
        Ok(())
    })?
}

struct RtssMap {
    view: *mut u8,
    handle: *mut c_void,
}

fn map_rtss() -> Option<RtssMap> {
    use windows_sys::Win32::System::Memory::{FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW};
    let wide: Vec<u16> = "RTSSSharedMemoryV2".encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide.as_ptr()) };
    if handle.is_null() {
        return None;
    }
    let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
    if view.Value.is_null() {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
        return None;
    }
    Some(RtssMap { view: view.Value.cast(), handle })
}

impl Drop for RtssMap {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.view.cast() });
            CloseHandle(self.handle);
        }
    }
}

fn u32_at(p: *const u8, off: usize) -> u32 {
    unsafe { std::ptr::read_unaligned(p.add(off).cast()) }
}

fn cstr_at(p: *const u8, off: usize, len: usize) -> String {
    let bytes = unsafe { std::slice::from_raw_parts(p.add(off), len) };
    let end = bytes.iter().position(|c| *c == 0).unwrap_or(0);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn read_frames() -> FrameStats {
    let Some(map) = map_rtss() else {
        return FrameStats::default();
    };
    let p = map.view;
    if u32_at(p, 0) != u32::from_le_bytes(*b"RTSS") {
        return FrameStats { rtss: false, ..Default::default() };
    }
    let version = u32_at(p, 4);
    if version < 0x0002_0000 {
        return FrameStats { rtss: true, ..Default::default() };
    }
    let app_stride = u32_at(p, 8) as usize;
    let app_off = u32_at(p, 12) as usize;
    let app_count = u32_at(p, 16) as usize;
    let mut frames = FrameStats { rtss: true, ..Default::default() };
    let foreground = if version >= 0x0002_0010 { u32_at(p, 64) as usize } else { 0 };
    let mut best: Option<(usize, u32)> = None;
    for i in 0..app_count.min(256) {
        let e = app_off + i * app_stride;
        let pid = u32_at(p, e);
        if pid == 0 {
            continue;
        }
        let frame_us = u32_at(p, e + 4 + 260 + 4 + 12);
        if frame_us > 0 && (best.is_none_or(|(_, us)| i == foreground || us > frame_us)) {
            best = Some((i, frame_us));
        }
    }
    if let Some((i, us)) = best {
        let e = app_off + i * app_stride;
        frames.frame_ms = Some(us as f64 / 1000.0);
        frames.fps = Some((1_000_000.0 / us as f64 * 10.0).round() / 10.0);
        frames.app = Some(cstr_at(p, e + 4, 260));
        let _ = foreground;
    }
    frames
}

fn write_osd(snap: &GpuSnapshot) -> Result<(), ()> {
    let Some(map) = map_rtss() else { return Err(()) };
    let p = map.view;
    if u32_at(p, 0) != u32::from_le_bytes(*b"RTSS") || u32_at(p, 4) < 0x0002_0000 {
        return Err(());
    }
    let stride = u32_at(p, 20) as usize;
    let off = u32_at(p, 24) as usize;
    let count = u32_at(p, 28) as usize;
    let gpu = snap.devices.first();
    let mut lines = Vec::new();
    if let Some(g) = gpu {
        let temp = g.temperature.map(|t| format!("{t}°C")).unwrap_or_else(|| "—".into());
        let util = g.utilization.map(|u| format!("{u}%")).unwrap_or_else(|| "—".into());
        let power = g.power_w.map(|w| format!("{w:.0} W")).unwrap_or_else(|| "—".into());
        lines.push(format!("{}   {temp}   {util}   {power}", g.name));
        let core = g.graphics_clock.map(|c| format!("{c} MHz")).unwrap_or_else(|| "—".into());
        let offc = g.core_offset.map(|o| if o >= 0 { format!("+{o}") } else { o.to_string() }).unwrap_or_default();
        let mem = g.memory_clock.map(|c| format!("{c} MHz")).unwrap_or_else(|| "—".into());
        lines.push(format!("Core {core} {offc}   Mem {mem}"));
        if let Some(fan) = g.fan_percent {
            let rpm = g.fan_rpm.map(|r| format!(" {r} rpm")).unwrap_or_default();
            lines.push(format!("Fan {fan}%{rpm}"));
        }
    }
    if let Some(fps) = snap.frames.fps {
        let low = snap.frames.one_percent_low.map(|v| format!("   1% {v:.0}")).unwrap_or_default();
        lines.push(format!("{fps:.0} FPS{low}"));
    }
    let text = lines.join("\n");
    let bytes = text.as_bytes();
    for slot in 1..count.min(8) {
        let e = off + slot * stride;
        let owner = cstr_at(p, e + 256, 256);
        if owner.is_empty() || owner == OSD_OWNER {
            unsafe {
                std::ptr::write_bytes(p.add(e), 0, 256);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(e), bytes.len().min(255));
                std::ptr::write_bytes(p.add(e + 256), 0, 256);
                std::ptr::copy_nonoverlapping(OSD_OWNER.as_ptr(), p.add(e + 256), OSD_OWNER.len());
            }
            let frame = u32_at(p, 32).wrapping_add(1);
            unsafe { std::ptr::write_unaligned(p.add(32).cast::<u32>(), frame) };
            return Ok(());
        }
    }
    Err(())
}

pub fn clear_osd() {
    let Some(map) = map_rtss() else { return };
    let p = map.view;
    if u32_at(p, 0) != u32::from_le_bytes(*b"RTSS") {
        return;
    }
    let stride = u32_at(p, 20) as usize;
    let off = u32_at(p, 24) as usize;
    let count = u32_at(p, 28) as usize;
    for slot in 1..count.min(8) {
        let e = off + slot * stride;
        if cstr_at(p, e + 256, 256) == OSD_OWNER {
            unsafe { std::ptr::write_bytes(p.add(e), 0, 512) };
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "reads the local NVIDIA driver"]
    fn read_local_gpu() {
        match super::read() {
            Ok(gpus) => println!("{}", serde_json::to_string_pretty(&gpus).unwrap()),
            Err(e) => panic!("{e}"),
        }
        let frames = super::read_frames();
        println!("frames {}", serde_json::to_string(&frames).unwrap());
    }
}
