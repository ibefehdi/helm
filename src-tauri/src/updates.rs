//! Driver and BIOS updates from GIGABYTE's own catalog (the one Control Center's Update Center uses).
//!
//! Driver matching mirrors `Driver_RAW_Table_Class.Get_Update_ListS`: present devices → PCITable →
//! IDtoDriver (OS column) → DriverTable, with the installed version taken from the device driver,
//! a registry value, or an Uninstall entry depending on DriverTable.Driver_Ver.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{self, AppState};

const DEFAULT_BASE: &str = "https://mb.download.gigabyte.com/FileList/Swhttp/DriverUpd/";
const BIOS_BASE: &str = "https://mb.download.gigabyte.com/FileList/BIOS/";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Edg/124.0.0.0";
const GCC_ROOT: &str = r"C:\Program Files\GIGABYTE\Control Center";
const CATALOG_FILES: [&str; 5] = ["DriverDesp.csv", "DriverTable.csv", "IDtoDriver.csv", "PCITable.csv", "Package.csv"];

type R<T> = Result<T, String>;

fn err(e: impl ToString) -> String {
    e.to_string()
}

// ---- shared state -----------------------------------------------------------------------------

#[derive(Default)]
struct Shared {
    drivers: Vec<DriverItem>,
    bios: Option<BiosInfo>,
}

fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(Default::default)
}

static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

struct BusyGuard;
impl BusyGuard {
    fn take() -> R<Self> {
        if BUSY.swap(true, Ordering::SeqCst) {
            return Err("Another update task is already running.".into());
        }
        CANCEL.store(false, Ordering::SeqCst);
        Ok(Self)
    }
}
impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn client() -> R<reqwest::Client> {
    reqwest::Client::builder().user_agent(USER_AGENT).connect_timeout(std::time::Duration::from_secs(15)).build().map_err(err)
}

fn base_url() -> String {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Gigabyte\AppCenter") {
            if let Ok(v) = key.get_value::<String, _>("tHttp") {
                let v = v.trim();
                if v.starts_with("https://") {
                    let v = v.replace("LiveUpdate4", "DriverUpd");
                    return if v.ends_with('/') { v } else { format!("{v}/") };
                }
            }
        }
    }
    DEFAULT_BASE.into()
}

fn work_dir(app: &AppHandle, sub: &str) -> R<PathBuf> {
    let dir = app.state::<AppState>().data_dir.join(sub);
    std::fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir)
}

// ---- text formats -----------------------------------------------------------------------------

fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse_ini(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_ascii_lowercase();
        } else if let Some((k, v)) = line.split_once('=') {
            out.entry(section.clone()).or_default().insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    out
}

/// The catalog CSVs are plain comma-separated with no quoting; the header row is kept as row 0.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| l.split(',').map(|c| c.trim().to_string()).collect()).collect()
}

fn col<'a>(row: &'a [String], i: usize) -> &'a str {
    row.get(i).map(String::as_str).unwrap_or("")
}

fn int(s: &str) -> Option<i64> {
    s.trim().parse().ok()
}

fn hex(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if s.is_empty() { None } else { u32::from_str_radix(s, 16).ok() }
}

async fn get_bytes(client: &reqwest::Client, url: &str) -> R<Vec<u8>> {
    let resp = client.get(url).send().await.map_err(err)?;
    if !resp.status().is_success() {
        return Err(format!("{} returned {}", url, resp.status()));
    }
    Ok(resp.bytes().await.map_err(err)?.to_vec())
}

async fn head_size(client: &reqwest::Client, url: &str) -> u64 {
    let Ok(resp) = client.head(url).send().await else { return 0 };
    if !resp.status().is_success() {
        return 0;
    }
    let h = resp.headers();
    let pick = |name: &str| h.get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    pick("x-amz-meta-cb-realsize").or_else(|| pick("content-length")).unwrap_or(0)
}

// ---- version rules ----------------------------------------------------------------------------

/// `Driver_RAW_Table_Class.Check_Version`: true when `new` is newer than `current`.
fn driver_newer(new: &str, current: &str) -> bool {
    if current == "N/A" {
        return true;
    }
    let a: Vec<&str> = new.split('.').collect();
    let b: Vec<&str> = current.split('.').collect();
    if a.len() != b.len() {
        return false;
    }
    let num = |v: &[&str]| -> Vec<i64> { v.iter().take(4).map(|s| s.trim().parse().unwrap_or(0)).collect() };
    num(&a) > num(&b)
}

/// `Bios_Lib.CompareBiosVer`: "F" followed by hex digits, so F1A > F12 > F11.
pub fn bios_cmp(web: &str, local: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    if web.is_empty() || local.is_empty() || web.len() > 4 || local.len() > 4 || !web.starts_with('F') || !local.starts_with('F') {
        return Equal;
    }
    let digits = |s: &str| s[1..].to_ascii_uppercase().chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>();
    if web.chars().any(|c| c.is_ascii_digit()) && local.chars().any(|c| c.is_ascii_digit()) {
        let w = u32::from_str_radix(&digits(web), 16).unwrap_or(0);
        let l = u32::from_str_radix(&digits(local), 16).unwrap_or(0);
        w.cmp(&l)
    } else {
        web[1..].cmp(&local[1..])
    }
}

// ---- device inventory -------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Device {
    dev_id: u32,
    sub_id: u32,
    version: String,
    name: String,
}

/// `Search_MB_Device_Class.Get_Dev_ID`: the 4 hex digits after DEV_ followed by the 4 before it (the vendor).
fn parse_hwid(id: &str) -> Option<(u32, u32)> {
    let up = id.to_ascii_uppercase();
    let after = |key: &str, len: usize| up.find(key).and_then(|i| up.get(i + key.len()..i + key.len() + len)).and_then(hex);
    let pci_like = up.starts_with(r"PCI\VEN_")
        || up.starts_with(r"HDAUDIO\FUNC_")
        || (up.starts_with(r"ACPI\VEN_") && (up.contains("_ITE&") || up.contains("PNP&DEV_0000")));
    if pci_like {
        let i = up.find("&DEV_")?;
        let dev = up.get(i + 5..i + 9)?;
        let ven = up.get(i.checked_sub(4)?..i)?;
        let dev_id = hex(&format!("{dev}{ven}"))?;
        return Some((dev_id, after("SUBSYS_", 8).unwrap_or(0)));
    }
    if up.starts_with(r"USB\VID_") || up.starts_with(r"HID\VID_") {
        return Some((after("VID_", 4)?, after("PID_", 4).unwrap_or(0)));
    }
    None
}

#[cfg(windows)]
fn devices() -> Vec<Device> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    const SPDRP_DEVICEDESC: u32 = 0;
    const SPDRP_HARDWAREID: u32 = 1;
    const SPDRP_DRIVER: u32 = 9;
    const SPDRP_FRIENDLYNAME: u32 = 12;

    let mut out = Vec::new();
    let set = unsafe { SetupDiGetClassDevsW(std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_ALLCLASSES) };
    if set == -1isize as HDEVINFO {
        return out;
    }
    let class_root = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Control\Class").ok();
    let prop = |info: &SP_DEVINFO_DATA, which: u32| -> Option<Vec<String>> {
        let mut buf = vec![0u16; 2048];
        let mut needed = 0u32;
        let ok = unsafe {
            SetupDiGetDeviceRegistryPropertyW(set, info, which, std::ptr::null_mut(), buf.as_mut_ptr().cast(), (buf.len() * 2) as u32, &mut needed)
        };
        if ok == 0 {
            return None;
        }
        let used = (needed as usize / 2).min(buf.len());
        Some(buf[..used].split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect())
    };

    let mut i = 0;
    loop {
        let mut info: SP_DEVINFO_DATA = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
        if unsafe { SetupDiEnumDeviceInfo(set, i, &mut info) } == 0 {
            break;
        }
        i += 1;
        let Some(ids) = prop(&info, SPDRP_HARDWAREID) else { continue };
        let Some((dev_id, sub_id)) = ids.first().and_then(|id| parse_hwid(id)) else { continue };
        let version = prop(&info, SPDRP_DRIVER)
            .and_then(|d| d.into_iter().next())
            .and_then(|key| class_root.as_ref()?.open_subkey(key).ok()?.get_value::<String, _>("DriverVersion").ok())
            .unwrap_or_else(|| "0.0.0.0".into());
        let name = prop(&info, SPDRP_FRIENDLYNAME)
            .or_else(|| prop(&info, SPDRP_DEVICEDESC))
            .and_then(|v| v.into_iter().next())
            .unwrap_or_default();
        out.push(Device { dev_id, sub_id, version, name });
    }
    unsafe { SetupDiDestroyDeviceInfoList(set) };
    out
}

#[cfg(not(windows))]
fn devices() -> Vec<Device> {
    Vec::new()
}

#[derive(Debug, Clone, Default)]
struct Package {
    driver_id: i64,
    display_name: String,
    kind: i64,
    view64: bool,
    path: String,
    value: String,
}

#[cfg(windows)]
fn reg_string(view64: bool, path: &str, name: &str) -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    let flags = KEY_READ | if view64 { KEY_WOW64_64KEY } else { KEY_WOW64_32KEY };
    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(path.trim_end_matches('\\'), flags).ok()?;
    key.get_value::<String, _>(name).ok().or_else(|| {
        (name == "AMD AM4/AM5 FCH RAID driver").then(|| key.get_value::<String, _>("AMD AM4 FCH RAID driver").ok()).flatten()
    })
}

#[cfg(windows)]
fn uninstall_version(view64: bool, display_name: &str) -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    let flags = KEY_READ | if view64 { KEY_WOW64_64KEY } else { KEY_WOW64_32KEY };
    let root = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", flags).ok()?;
    root.enum_keys().flatten().find_map(|name| {
        let key = root.open_subkey_with_flags(&name, flags).ok()?;
        (key.get_value::<String, _>("DisplayName").ok()? == display_name).then(|| key.get_value::<String, _>("DisplayVersion").ok()).flatten()
    })
}

#[cfg(not(windows))]
fn reg_string(_: bool, _: &str, _: &str) -> Option<String> {
    None
}
#[cfg(not(windows))]
fn uninstall_version(_: bool, _: &str) -> Option<String> {
    None
}

/// `Get_Ver_string`: Driver_Ver 99 reads a registry value, 98 searches Uninstall, anything else uses the device driver.
fn installed_version(mode: i64, pkg: Option<&Package>, device_version: &str) -> String {
    match (mode, pkg) {
        (99, Some(p)) if !p.path.is_empty() && !p.value.is_empty() => reg_string(p.view64, &p.path, &p.value).unwrap_or_else(|| "N/A".into()),
        (98, Some(p)) if !p.display_name.is_empty() => uninstall_version(p.view64, &p.display_name).unwrap_or_else(|| "N/A".into()),
        (99 | 98, _) if device_version.is_empty() => "N/A".into(),
        _ => device_version.to_string(),
    }
}

// ---- catalog ----------------------------------------------------------------------------------

struct Catalog {
    revision: String,
    table: Vec<Vec<String>>,
    desp: HashMap<i64, (String, String)>,
    id_to_driver: Vec<Vec<String>>,
    pci: Vec<Vec<String>>,
    packages: Vec<Package>,
}

fn is_amd() -> bool {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;
        if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") {
            if let Ok(name) = k.get_value::<String, _>("ProcessorNameString") {
                return name.to_ascii_uppercase().contains("AMD");
            }
        }
    }
    false
}

/// `GBT_MB_UpdateControl` picks one platform package list next to the shared Package.csv.
fn platform_package_file(model: &str) -> Option<&'static str> {
    if is_amd() {
        return Some("Package-AMD.csv");
    }
    let m = model.to_ascii_uppercase();
    let has = |chips: &[&str]| chips.iter().any(|c| m.contains(c));
    if has(&["Z890", "B860", "H810"]) {
        Some("Package-Intel-800.csv")
    } else if has(&["W790"]) {
        Some("Package-Intel-W790.csv")
    } else if has(&["Z790", "B760", "H770"]) {
        Some("Package-Intel-700.csv")
    } else if has(&["Z690", "B660", "H610", "H670"]) {
        Some("Package-Intel-600.csv")
    } else {
        None
    }
}

fn parse_packages(text: &str) -> Vec<Package> {
    parse_csv(text)
        .into_iter()
        .skip(1)
        .filter_map(|r| {
            Some(Package {
                driver_id: int(col(&r, 0))?,
                display_name: col(&r, 1).to_string(),
                kind: int(col(&r, 2)).unwrap_or(0),
                view64: col(&r, 3) != "512",
                path: col(&r, 4).replace(r"\\", r"\"),
                value: col(&r, 5).to_string(),
            })
        })
        .collect()
}

async fn load_catalog(app: &AppHandle, client: &reqwest::Client, base: &str, model: &str) -> R<Catalog> {
    let ini = decode_text(&get_bytes(client, &format!("{base}Drvdata/ver2.ini")).await?);
    let revision = parse_ini(&ini).get("info").and_then(|s| s.get("path")).cloned().ok_or("The driver catalog index has no revision.")?;
    let dir = work_dir(app, &format!("drvdata/{revision}"))?;

    let mut files: Vec<&str> = CATALOG_FILES.to_vec();
    if let Some(extra) = platform_package_file(model) {
        files.push(extra);
    }
    let mut texts = HashMap::new();
    for name in files {
        let path = dir.join(name);
        let bytes = match std::fs::read(&path) {
            Ok(b) if !b.is_empty() => b,
            _ => match get_bytes(client, &format!("{base}Drvdata/{revision}/{name}")).await {
                Ok(b) => {
                    let _ = std::fs::write(&path, &b);
                    b
                }
                Err(e) if name.starts_with("Package-") => {
                    eprintln!("optional catalog file skipped: {e}");
                    continue;
                }
                Err(e) => return Err(e),
            },
        };
        texts.insert(name, decode_text(&bytes));
    }

    let desp = parse_csv(&texts["DriverDesp.csv"])
        .into_iter()
        .skip(1)
        .filter_map(|r| Some((int(col(&r, 0))?, (col(&r, 1).to_string(), col(&r, 2).to_string()))))
        .collect();
    let mut packages = parse_packages(&texts["Package.csv"]);
    for (name, text) in &texts {
        if name.starts_with("Package-") {
            packages.extend(parse_packages(text));
        }
    }
    Ok(Catalog {
        revision,
        table: parse_csv(&texts["DriverTable.csv"]),
        desp,
        id_to_driver: parse_csv(&texts["IDtoDriver.csv"]),
        pci: parse_csv(&texts["PCITable.csv"]),
        packages,
    })
}

fn os_column() -> usize {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;
        if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") {
            if let Some(build) = k.get_value::<String, _>("CurrentBuildNumber").ok().and_then(|b| b.parse::<u32>().ok()) {
                return if build < 22000 { 16 } else { 3 };
            }
        }
    }
    3
}

// ---- driver scan ------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverItem {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub category: String,
    pub installed: String,
    pub latest: String,
    pub outdated: bool,
    pub size: u64,
    pub url: String,
    pub file_name: String,
    pub devices: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverScan {
    pub revision: String,
    pub scanned_at: i64,
    pub device_count: usize,
    pub items: Vec<DriverItem>,
}

fn category(path: &str) -> String {
    let first = path.split(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("Other");
    match first.to_ascii_lowercase().as_str() {
        "chipset" => "Chipset",
        "lan" | "network" => "Network",
        "wlan" | "wifi" | "wireless" => "Wireless",
        "bt" | "bluetooth" => "Bluetooth",
        "audio" | "sound" => "Audio",
        "vga" | "graphic" | "graphics" => "Graphics",
        "sata" | "raid" | "storage" => "Storage",
        "usb" => "USB",
        _ => first,
    }
    .to_string()
}

fn build_items(cat: &Catalog, devices: &[Device], base: &str) -> Vec<DriverItem> {
    let os = os_column();
    let table_row = |id: i64| cat.table.iter().skip(1).find(|r| int(col(r, 0)) == Some(id));
    let package = |id: i64| cat.packages.iter().find(|p| p.driver_id == id);
    let make = |row: &[String], installed: String, device: Option<&str>, source: &str| -> DriverItem {
        let id = int(col(row, 0)).unwrap_or(0);
        let latest = col(row, 13).to_string();
        let (name, description) = int(col(row, 2)).and_then(|d| cat.desp.get(&d).cloned()).unwrap_or_default();
        let rel = format!("{}{}_{}_{}", col(row, 3), id, latest, col(row, 4)).replace('\\', "/").to_ascii_uppercase();
        let installed = if installed == "0.0.0.0" { "N/A".into() } else { installed };
        DriverItem {
            id,
            name: if name.is_empty() { format!("Driver {id}") } else { name },
            description,
            category: category(col(row, 3)),
            outdated: driver_newer(&latest, &installed),
            installed,
            latest,
            size: 0,
            url: format!("{base}{rel}"),
            file_name: rel.rsplit('/').next().unwrap_or_default().to_string(),
            devices: device.filter(|d| !d.is_empty()).map(|d| vec![d.to_string()]).unwrap_or_default(),
            source: source.into(),
        }
    };

    let mut items: BTreeMap<i64, DriverItem> = BTreeMap::new();
    for dev in devices {
        // First matching PCITable row wins, exactly like GCC's `break` after a hit.
        let Some(pci_num) = cat.pci.iter().skip(1).find_map(|r| {
            let id = hex(col(r, 1))?;
            let special = int(col(r, 2)).unwrap_or(0) != 0;
            let hit = if special { id == dev.dev_id && hex(col(r, 3)).is_some_and(|s| s == dev.sub_id) } else { id == dev.dev_id };
            hit.then(|| int(col(r, 5))).flatten()
        }) else {
            continue;
        };
        for map in cat.id_to_driver.iter().skip(1).filter(|r| int(col(r, 1)) == Some(pci_num) && col(r, os) == "1") {
            let Some(driver_id) = int(col(map, 2)) else { continue };
            let Some(row) = table_row(driver_id) else { continue };
            let mode = int(col(row, 5)).unwrap_or(0);
            let installed = installed_version(mode, package(driver_id), &dev.version);
            match items.get_mut(&driver_id) {
                Some(existing) => {
                    if !dev.name.is_empty() && !existing.devices.contains(&dev.name) {
                        existing.devices.push(dev.name.clone());
                    }
                }
                None => {
                    items.insert(driver_id, make(row, installed, Some(&dev.name), "device"));
                }
            }
        }
    }

    // Drivers GIGABYTE tracks by registry rather than by device. The package lists span every board on the
    // platform, so only ones already installed here are relevant; GCC hides those, Helm offers their updates.
    // Applications (PackageType 1) are skipped: that list is mostly bundled third-party software.
    for pkg in cat.packages.iter().filter(|p| p.kind == 2) {
        if items.contains_key(&pkg.driver_id) {
            continue;
        }
        let Some(row) = table_row(pkg.driver_id) else { continue };
        let mode = int(col(row, 5)).unwrap_or(99);
        let installed = installed_version(if mode == 98 { 98 } else { 99 }, Some(pkg), "");
        if installed == "N/A" || installed.is_empty() {
            continue;
        }
        items.insert(pkg.driver_id, make(row, installed, None, "registry"));
    }
    items.into_values().collect()
}

pub async fn scan_drivers(app: &AppHandle) -> R<DriverScan> {
    let _busy = BusyGuard::take()?;
    let client = client()?;
    let base = base_url();
    let board = board_info(app).await.unwrap_or_default();
    let _ = app.emit("updates-progress", json!({ "phase": "catalog" }));
    let catalog = load_catalog(app, &client, &base, &board.model).await?;
    let _ = app.emit("updates-progress", json!({ "phase": "devices" }));
    let devs = tokio::task::spawn_blocking(devices).await.map_err(err)?;
    let mut items = build_items(&catalog, &devs, &base);

    let _ = app.emit("updates-progress", json!({ "phase": "sizes", "total": items.len() }));
    let targets: Vec<(i64, String)> = items.iter().map(|i| (i.id, i.url.clone())).collect();
    let sizes: HashMap<i64, u64> = futures_util::stream::iter(targets)
        .map(|(id, url)| {
            let client = client.clone();
            async move { (id, head_size(&client, &url).await) }
        })
        .buffer_unordered(8)
        .collect()
        .await;
    for item in &mut items {
        item.size = sizes.get(&item.id).copied().unwrap_or(0);
    }
    // GCC drops entries whose installer is missing from the CDN; keep them visible but not installable.
    items.retain(|i| i.size > 0 || !i.outdated);
    items.sort_by(|a, b| b.outdated.cmp(&a.outdated).then(a.category.cmp(&b.category)).then(a.name.cmp(&b.name)));

    shared().lock().unwrap().drivers = items.clone();
    Ok(DriverScan { revision: catalog.revision, scanned_at: chrono::Utc::now().timestamp_millis(), device_count: devs.len(), items })
}

// ---- downloads --------------------------------------------------------------------------------

async fn download(app: &AppHandle, client: &reqwest::Client, url: &str, dest: &Path, expected: u64, tag: serde_json::Value) -> R<()> {
    if expected > 0 && std::fs::metadata(dest).map(|m| m.len() == expected).unwrap_or(false) {
        return Ok(());
    }
    let part = dest.with_extension("part");
    let resp = client.get(url).send().await.map_err(err)?;
    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(expected);
    let mut file = tokio::fs::File::create(&part).await.map_err(err)?;
    let mut stream = resp.bytes_stream();
    let mut received = 0u64;
    let mut last_emit = 0u64;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        if CANCEL.load(Ordering::SeqCst) {
            drop(file);
            let _ = std::fs::remove_file(&part);
            return Err("Cancelled.".into());
        }
        let chunk = chunk.map_err(err)?;
        file.write_all(&chunk).await.map_err(err)?;
        received += chunk.len() as u64;
        if received - last_emit > 512 * 1024 || received == total {
            last_emit = received;
            let mut payload = tag.clone();
            payload["phase"] = "download".into();
            payload["received"] = received.into();
            payload["total"] = total.into();
            let _ = app.emit("updates-progress", payload);
        }
    }
    file.flush().await.map_err(err)?;
    drop(file);
    if expected > 0 && received != expected {
        let _ = std::fs::remove_file(&part);
        return Err(format!("Download was truncated ({received} of {expected} bytes)."));
    }
    std::fs::rename(&part, dest).map_err(err)
}

fn hidden_command(program: &str) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    pub valid: bool,
    pub status: String,
    pub signer: String,
}

/// The catalog carries no hashes, so the Authenticode signature is the integrity check.
fn authenticode(path: &Path) -> Signature {
    let script = format!(
        "$s = Get-AuthenticodeSignature -LiteralPath '{}'; \"$($s.Status)|$(if ($s.SignerCertificate) {{ $s.SignerCertificate.GetNameInfo('SimpleName', $false) }})\"",
        path.display().to_string().replace('\'', "''")
    );
    let out = hidden_command("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let (status, signer) = text.split_once('|').unwrap_or((&text, ""));
    Signature { valid: status == "Valid", status: status.to_string(), signer: signer.trim().to_string() }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub id: i64,
    pub name: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub reboot: bool,
    pub signer: String,
    pub message: String,
}

pub async fn install_drivers(app: &AppHandle, ids: Vec<i64>) -> R<Vec<InstallResult>> {
    let _busy = BusyGuard::take()?;
    let client = client()?;
    let dir = work_dir(app, "downloads/drivers")?;
    let queue: Vec<DriverItem> = {
        let s = shared().lock().unwrap();
        ids.iter().filter_map(|id| s.drivers.iter().find(|d| d.id == *id).cloned()).collect()
    };
    if queue.is_empty() {
        return Err("Nothing selected. Scan for updates first.".into());
    }

    let mut results = Vec::new();
    for (n, item) in queue.iter().enumerate() {
        let tag = json!({ "id": item.id, "index": n, "count": queue.len(), "name": item.name });
        let mut result = InstallResult { id: item.id, name: item.name.clone(), ok: false, exit_code: None, reboot: false, signer: String::new(), message: String::new() };
        if CANCEL.load(Ordering::SeqCst) {
            result.message = "Cancelled.".into();
            results.push(result);
            continue;
        }
        let dest = dir.join(&item.file_name);
        if let Err(e) = download(app, &client, &item.url, &dest, item.size, tag.clone()).await {
            result.message = e;
            results.push(result);
            continue;
        }

        let mut verify = tag.clone();
        verify["phase"] = "verify".into();
        let _ = app.emit("updates-progress", verify);
        let path = dest.clone();
        let sig = tokio::task::spawn_blocking(move || authenticode(&path)).await.map_err(err)?;
        result.signer = sig.signer.clone();
        if !sig.valid {
            let _ = std::fs::remove_file(&dest);
            result.message = format!("Refused to run: the installer's signature is {}.", if sig.status.is_empty() { "missing" } else { &sig.status });
            results.push(result);
            continue;
        }

        let mut installing = tag.clone();
        installing["phase"] = "install".into();
        installing["signer"] = sig.signer.clone().into();
        let _ = app.emit("updates-progress", installing);
        let exe = dest.clone();
        let status = tokio::task::spawn_blocking(move || {
            let mut c = std::process::Command::new(&exe);
            if let Some(parent) = exe.parent() {
                c.current_dir(parent);
            }
            c.status()
        })
        .await
        .map_err(err)?;
        match status {
            Ok(s) => {
                let code = s.code();
                result.exit_code = code;
                result.reboot = matches!(code, Some(1641 | 3010));
                result.ok = matches!(code, Some(0 | 1641 | 3010));
                result.message = match code {
                    Some(0) => "Installed.".into(),
                    Some(1641 | 3010) => "Installed. A restart is needed to finish.".into(),
                    Some(1602) => "The installer was cancelled.".into(),
                    Some(c) => format!("The installer exited with code {c}."),
                    None => "The installer was terminated.".into(),
                };
            }
            Err(e) => result.message = format!("Could not start the installer: {e}"),
        }
        let mut done = tag.clone();
        done["phase"] = if result.ok { "done" } else { "failed" }.into();
        done["message"] = result.message.clone().into();
        let _ = app.emit("updates-progress", done);
        results.push(result);
    }
    Ok(results)
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

// ---- BIOS -------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BoardInfo {
    model: String,
    part_number: Option<String>,
    bios_version: String,
    bios_date: Option<String>,
}

async fn board_info(app: &AppHandle) -> R<BoardInfo> {
    let raw = state::call(app, "board.fwinfo", json!({})).await.map_err(err)?;
    serde_json::from_value(raw).map_err(err)
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BiosInfo {
    pub model: String,
    pub part_number: Option<String>,
    pub installed: String,
    pub installed_date: Option<String>,
    pub latest: Option<String>,
    pub update_available: bool,
    pub url: Option<String>,
    pub size: u64,
    pub downloaded: Option<String>,
    pub note: Option<String>,
}

pub async fn check_bios(app: &AppHandle) -> R<BiosInfo> {
    let board = board_info(app).await?;
    let mut info = BiosInfo {
        model: board.model.clone(),
        part_number: board.part_number.clone(),
        installed: board.bios_version.clone(),
        installed_date: board.bios_date.clone(),
        ..Default::default()
    };
    let Some(part) = board.part_number.clone().filter(|p| !p.is_empty()) else {
        info.note = Some("The firmware did not report a GIGABYTE part number, so the BIOS catalog can't be matched.".into());
        shared().lock().unwrap().bios = Some(info.clone());
        return Ok(info);
    };
    if part.eq_ignore_ascii_case("8ASTP001") {
        info.note = Some("GIGABYTE excludes this board from online BIOS updates.".into());
        return Ok(info);
    }

    let client = client()?;
    let base = base_url();
    let mut ini = None;
    for name in ["ver2.ini", "Ver2.ini", "VER2.ini"] {
        if let Ok(b) = get_bytes(&client, &format!("{base}BIOS/{name}")).await {
            ini = Some(parse_ini(&decode_text(&b)));
            break;
        }
    }
    let ini = ini.ok_or("Couldn't download GIGABYTE's BIOS index.")?;
    let latest = ini.get(&part.to_ascii_lowercase()).and_then(|s| s.get("biosver0")).cloned();
    let Some(latest) = latest else {
        info.note = Some(format!("GIGABYTE's BIOS index has no entry for part number {part}."));
        shared().lock().unwrap().bios = Some(info.clone());
        return Ok(info);
    };

    let model_slug = board.model.replace(' ', "-").to_ascii_lowercase();
    let url = format!("{BIOS_BASE}mb_bios_{model_slug}_{}_{}.zip", part.to_ascii_lowercase(), latest.to_ascii_lowercase());
    info.size = head_size(&client, &url).await;
    info.update_available = info.size > 0 && (bios_cmp(&latest, &board.bios_version).is_gt() || board.bios_version == "T0d");
    if info.size == 0 {
        info.note = Some("The BIOS package for this version isn't on GIGABYTE's server yet.".into());
    }
    let image = work_dir(app, &format!("downloads/bios/{}", latest.to_ascii_lowercase()))?;
    info.downloaded = find_image(&image, &latest).map(|p| p.display().to_string());
    info.latest = Some(latest);
    info.url = Some(url);
    shared().lock().unwrap().bios = Some(info.clone());
    Ok(info)
}

fn find_image(dir: &Path, version: &str) -> Option<PathBuf> {
    let ext = version.to_ascii_uppercase();
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.extension().map(|e| e.to_string_lossy().to_ascii_uppercase() == ext).unwrap_or(false)
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BiosDownload {
    pub version: String,
    pub image: String,
    pub folder: String,
    pub size: u64,
}

/// Downloads and unpacks the BIOS zip, insisting on exactly one image whose extension is the version
/// (GIGABYTE names them e.g. `X870EAGLEWIFI7.F12`); the zip's CRCs are checked during extraction.
pub async fn download_bios(app: &AppHandle) -> R<BiosDownload> {
    let _busy = BusyGuard::take()?;
    let info = shared().lock().unwrap().bios.clone().ok_or("Check for a BIOS update first.")?;
    let (Some(url), Some(version)) = (info.url.clone(), info.latest.clone()) else {
        return Err("No BIOS package is available for this board.".into());
    };
    let client = client()?;
    let folder = work_dir(app, &format!("downloads/bios/{}", version.to_ascii_lowercase()))?;
    let zip_path = folder.join(url.rsplit('/').next().unwrap_or("bios.zip"));
    download(app, &client, &url, &zip_path, info.size, json!({ "id": "bios", "name": format!("BIOS {version}") })).await?;
    let _ = app.emit("updates-progress", json!({ "id": "bios", "phase": "verify" }));

    let (out_dir, zip_file, ver) = (folder.clone(), zip_path.clone(), version.clone());
    let image = tokio::task::spawn_blocking(move || extract_image(&zip_file, &out_dir, &ver)).await.map_err(err)??;

    let size = std::fs::metadata(&image).map(|m| m.len()).unwrap_or(0);
    let result = BiosDownload { version, image: image.display().to_string(), folder: folder.display().to_string(), size };
    if let Some(b) = shared().lock().unwrap().bios.as_mut() {
        b.downloaded = Some(result.image.clone());
    }
    let _ = app.emit("updates-progress", json!({ "id": "bios", "phase": "done" }));
    Ok(result)
}

fn extract_image(zip_file: &Path, out_dir: &Path, version: &str) -> R<PathBuf> {
    let ext = version.to_ascii_uppercase();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_file).map_err(err)?).map_err(|e| format!("The BIOS package is corrupt: {e}"))?;
    let mut image = None;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(err)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        if entry.is_dir() {
            continue;
        }
        let target = out_dir.join(rel.file_name().unwrap_or_default());
        let mut out = std::fs::File::create(&target).map_err(err)?;
        // io::copy surfaces a CRC mismatch as an error once the entry is fully read.
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("The BIOS package failed its integrity check: {e}"))?;
        if target.extension().map(|e| e.to_string_lossy().to_ascii_uppercase() == ext).unwrap_or(false) {
            if image.is_some() {
                return Err("The BIOS package contains more than one image; refusing to guess.".into());
            }
            image = Some(target);
        }
    }
    let image = image.ok_or_else(|| format!("No .{ext} image was found in the BIOS package."))?;
    let len = std::fs::metadata(&image).map_err(err)?.len();
    if len < 4 * 1024 * 1024 || len % 4096 != 0 {
        return Err(format!("The BIOS image has an unexpected size ({len} bytes)."));
    }
    Ok(image)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbDrive {
    pub letter: String,
    pub label: String,
    pub file_system: String,
    pub size: u64,
    pub free: u64,
    pub fat32: bool,
}

#[cfg(windows)]
pub fn usb_drives() -> Vec<UsbDrive> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    const DRIVE_REMOVABLE: u32 = 2;
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = char::from(b'A' + i as u8);
        let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain(Some(0)).collect();
        if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_REMOVABLE {
            continue;
        }
        let mut label = [0u16; 261];
        let mut fs = [0u16; 261];
        let ok = unsafe {
            GetVolumeInformationW(root.as_ptr(), label.as_mut_ptr(), 261, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), 261)
        };
        if ok == 0 {
            continue;
        }
        let (mut free, mut total) = (0u64, 0u64);
        unsafe { GetDiskFreeSpaceExW(root.as_ptr(), std::ptr::null_mut(), &mut total, &mut free) };
        let s = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
        let file_system = s(&fs);
        out.push(UsbDrive { letter: letter.to_string(), label: s(&label), fat32: file_system.eq_ignore_ascii_case("FAT32"), file_system, size: total, free });
    }
    out
}

#[cfg(not(windows))]
pub fn usb_drives() -> Vec<UsbDrive> {
    Vec::new()
}

/// Q-Flash reads the image from the root of a FAT32 stick; this is the hand-off GIGABYTE recommends.
pub fn copy_bios_to_usb(letter: &str) -> R<String> {
    let image = shared().lock().unwrap().bios.as_ref().and_then(|b| b.downloaded.clone()).ok_or("Download the BIOS first.")?;
    let drive = usb_drives().into_iter().find(|d| d.letter.eq_ignore_ascii_case(letter)).ok_or("That USB drive is no longer connected.")?;
    if !drive.fat32 {
        return Err(format!("{}: is formatted as {}; Q-Flash needs FAT32.", drive.letter, drive.file_system));
    }
    let src = PathBuf::from(&image);
    let size = std::fs::metadata(&src).map_err(err)?.len();
    if drive.free < size {
        return Err("Not enough free space on the USB drive.".into());
    }
    let dest = PathBuf::from(format!("{}:\\", drive.letter)).join(src.file_name().ok_or("bad image path")?);
    std::fs::copy(&src, &dest).map_err(err)?;
    if std::fs::read(&src).map_err(err)? != std::fs::read(&dest).map_err(err)? {
        let _ = std::fs::remove_file(&dest);
        return Err("The copy on the USB drive doesn't match the download.".into());
    }
    Ok(dest.display().to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashReadiness {
    pub flasher_found: bool,
    pub on_ac_power: Option<bool>,
    pub battery_percent: Option<u8>,
    pub bitlocker_on: Option<bool>,
    pub image: Option<String>,
}

fn flasher() -> PathBuf {
    PathBuf::from(GCC_ROOT).join(r"Lib\Update_Center\FlashBios.exe")
}

fn bitlocker_on() -> Option<bool> {
    let out = hidden_command("manage-bde").args(["-status", "C:"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    if text.contains("protection on") {
        Some(true)
    } else if text.contains("protection off") {
        Some(false)
    } else {
        None
    }
}

#[cfg(windows)]
fn power() -> (Option<bool>, Option<u8>) {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return (None, None);
    }
    let ac = match s.ACLineStatus {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    let pct = (s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent);
    (ac, pct)
}

#[cfg(not(windows))]
fn power() -> (Option<bool>, Option<u8>) {
    (None, None)
}

pub fn flash_readiness() -> FlashReadiness {
    let (ac, pct) = power();
    FlashReadiness {
        flasher_found: flasher().exists(),
        on_ac_power: ac,
        battery_percent: pct,
        bitlocker_on: bitlocker_on(),
        image: shared().lock().unwrap().bios.as_ref().and_then(|b| b.downloaded.clone()),
    }
}

/// Hands the verified image to GIGABYTE's own flasher, which reboots into the update itself.
/// Helm never writes the flash chip; it only enforces the preconditions GCC skips.
pub fn flash_bios(confirm_model: &str, suspend_bitlocker: bool) -> R<()> {
    let bios = shared().lock().unwrap().bios.clone().ok_or("Check for a BIOS update first.")?;
    let image = bios.downloaded.clone().ok_or("Download and verify the BIOS first.")?;
    if confirm_model.trim() != bios.model.trim() {
        return Err("The typed model name doesn't match this board.".into());
    }
    if !flasher().exists() {
        return Err("GIGABYTE's flasher (FlashBios.exe) isn't installed with Control Center. Use the USB / Q-Flash method instead.".into());
    }
    let ready = flash_readiness();
    if ready.on_ac_power == Some(false) {
        return Err("Connect AC power before flashing.".into());
    }
    if ready.bitlocker_on == Some(true) {
        if !suspend_bitlocker {
            return Err("BitLocker is on. Suspend it for one reboot first, or the drive will ask for its recovery key.".into());
        }
        let out = hidden_command("manage-bde").args(["-protectors", "-disable", "C:", "-RebootCount", "1"]).output().map_err(err)?;
        if !out.status.success() {
            return Err(format!("Couldn't suspend BitLocker: {}", String::from_utf8_lossy(&out.stdout).trim()));
        }
    }
    std::process::Command::new(flasher())
        .args(["/f", &image])
        .current_dir(flasher().parent().unwrap())
        .spawn()
        .map_err(|e| format!("Couldn't start FlashBios.exe: {e}"))?;
    Ok(())
}

pub fn open_folder_target(app: &AppHandle) -> R<PathBuf> {
    work_dir(app, "downloads")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_rules() {
        assert!(bios_cmp("F12", "F11").is_gt());
        assert!(bios_cmp("F1A", "F12").is_gt());
        assert!(bios_cmp("F9", "F10").is_lt());
        assert!(driver_newer("1.2.3.4", "N/A"));
        assert!(driver_newer("10.1.2.9", "10.1.2.8"));
        assert!(!driver_newer("10.1.2", "10.1.2.8"));
        assert!(!driver_newer("8.03.25.247", "8.03.25.247"));
        assert_eq!(parse_hwid(r"PCI\VEN_10EC&DEV_8125&SUBSYS_E0001458&REV_05"), Some((0x8125_10EC, 0xE000_1458)));
        assert_eq!(parse_hwid(r"USB\VID_048D&PID_5711&REV_0001"), Some((0x048D, 0x5711)));
    }

    #[tokio::test]
    #[ignore = "downloads this board's BIOS package from GIGABYTE"]
    async fn live_bios_pipeline() {
        let (model, part, installed) = ("X870 EAGLE WIFI7", "8ARPL325", "F11");
        let client = client().unwrap();
        let ini = parse_ini(&decode_text(&get_bytes(&client, &format!("{}BIOS/ver2.ini", base_url())).await.unwrap()));
        let latest = ini[&part.to_ascii_lowercase()]["biosver0"].clone();
        let url = format!("{BIOS_BASE}mb_bios_{}_{}_{}.zip", model.replace(' ', "-").to_ascii_lowercase(), part.to_ascii_lowercase(), latest.to_ascii_lowercase());
        let size = head_size(&client, &url).await;
        println!("latest={latest} newer_than_{installed}={:?} url={url} size={size}", bios_cmp(&latest, installed));
        assert!(size > 0);
        let dir = std::env::temp_dir().join("helm-bios-test");
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("bios.zip");
        std::fs::write(&zip_path, get_bytes(&client, &url).await.unwrap()).unwrap();
        let image = extract_image(&zip_path, &dir, &latest).unwrap();
        println!("image={} bytes={}", image.display(), std::fs::metadata(&image).unwrap().len());
        println!("signature check on the flasher: {:?}", authenticode(&PathBuf::from(GCC_ROOT).join(r"Lib\Update_Center\FlashBios.exe")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[ignore = "hits GIGABYTE's CDN and enumerates local devices"]
    async fn live_driver_match() {
        let client = client().unwrap();
        let base = base_url();
        let ini = decode_text(&get_bytes(&client, &format!("{base}Drvdata/ver2.ini")).await.unwrap());
        let rev = parse_ini(&ini)["info"]["path"].clone();
        let mut texts = HashMap::new();
        for name in CATALOG_FILES.iter().copied().chain(["Package-AMD.csv"]) {
            texts.insert(name, decode_text(&get_bytes(&client, &format!("{base}Drvdata/{rev}/{name}")).await.unwrap()));
        }
        let mut packages = parse_packages(&texts["Package.csv"]);
        packages.extend(parse_packages(&texts["Package-AMD.csv"]));
        let cat = Catalog {
            revision: rev,
            table: parse_csv(&texts["DriverTable.csv"]),
            desp: parse_csv(&texts["DriverDesp.csv"])
                .into_iter()
                .skip(1)
                .filter_map(|r| Some((int(col(&r, 0))?, (col(&r, 1).to_string(), col(&r, 2).to_string()))))
                .collect(),
            id_to_driver: parse_csv(&texts["IDtoDriver.csv"]),
            pci: parse_csv(&texts["PCITable.csv"]),
            packages,
        };
        let devs = devices();
        println!("devices: {}", devs.len());
        for item in build_items(&cat, &devs, &base) {
            let size = head_size(&client, &item.url).await;
            println!("{:>4} {:<10} {:<45} installed={:<16} latest={:<16} outdated={} size={} src={} {:?}", item.id, item.category, item.name, item.installed, item.latest, item.outdated, size, item.source, item.devices);
        }
    }
}