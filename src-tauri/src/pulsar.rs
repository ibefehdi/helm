//! Pulsar wireless mice (VID 0x3710) through the vendor HID collection that Pulsar's own
//! software talks to. Helm speaks the same HID report protocol; it does not load Pulsar's DLL.
//!
//! Every request and reply is a 17-byte report laid out as
//! `[0x08, cmd, 0, addr_hi, addr_lo, len, data[10], check]`, where `check = 0x55 - sum(first 16)`.
//! Settings live in the mouse's flash; each stored value is followed by its own check byte
//! (`0x55 - sum(value bytes)`). A write (cmd 0x07) takes effect immediately.

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid,
    HidD_GetPreparsedData, HidD_GetProductString, HidP_GetCaps,
};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};

pub const VENDOR_ID: u16 = 0x3710;
const REPORT_LEN: usize = 17;
/// Pulsar's command collection lives on this HID usage page (usage 0x02).
const COMMAND_USAGE_PAGE: u16 = 0xFF02;

pub const CMD_ONLINE: u8 = 0x03;
pub const CMD_BATTERY: u8 = 0x04;
pub const CMD_WRITE: u8 = 0x07;
pub const CMD_READ: u8 = 0x08;
pub const CMD_VERSION: u8 = 0x12;

/// `0x55 - sum(bytes)`, Pulsar's one-byte checksum over a slice.
pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0x55u8, |acc, b| acc.wrapping_sub(*b))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

unsafe impl Send for OwnedHandle {}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub path: String,
    pub product_id: u16,
    pub product: String,
}

/// Enumerate every HID interface whose vendor id is Pulsar's. Only the vendor command collection
/// accepts the 17-byte protocol; `open` filters by report length, so returning all is fine.
pub fn enumerate() -> Vec<Candidate> {
    let mut guid = unsafe { std::mem::zeroed() };
    unsafe { HidD_GetHidGuid(&mut guid) };
    let set = unsafe { SetupDiGetClassDevsW(&guid, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) };
    // HDEVINFO is an isize handle; its invalid sentinel is -1.
    if set == -1 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut index = 0u32;
    loop {
        let mut iface: SP_DEVICE_INTERFACE_DATA = unsafe { std::mem::zeroed() };
        iface.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        if unsafe { SetupDiEnumDeviceInterfaces(set, null(), &guid, index, &mut iface) } == 0 {
            break;
        }
        index += 1;
        let mut needed = 0u32;
        unsafe { SetupDiGetDeviceInterfaceDetailW(set, &iface, null_mut(), 0, &mut needed, null_mut()) };
        if needed == 0 {
            continue;
        }
        let mut buf = vec![0u8; needed as usize];
        let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        // cbSize is the fixed header size, not the whole allocation (4 + 2 on 64-bit).
        unsafe { (*detail).cbSize = 8 };
        if unsafe { SetupDiGetDeviceInterfaceDetailW(set, &iface, detail, needed, null_mut(), null_mut()) } == 0 {
            continue;
        }
        let path_ptr = unsafe { (*detail).DevicePath.as_ptr() };
        let mut len = 0usize;
        while unsafe { *path_ptr.add(len) } != 0 {
            len += 1;
        }
        let path = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(path_ptr, len) });
        if let Some(candidate) = inspect(&path) {
            out.push(candidate);
        }
    }
    unsafe { SetupDiDestroyDeviceInfoList(set) };
    out
}

fn inspect(path: &str) -> Option<Candidate> {
    let wide = wide(path);
    let handle = unsafe {
        CreateFileW(wide.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut())
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let handle = OwnedHandle(handle);
    let mut attrs: HIDD_ATTRIBUTES = unsafe { std::mem::zeroed() };
    attrs.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
    if unsafe { HidD_GetAttributes(handle.0, &mut attrs) } == 0 || attrs.VendorID != VENDOR_ID {
        return None;
    }
    let mut product = String::new();
    let mut name = [0u16; 128];
    if unsafe { HidD_GetProductString(handle.0, name.as_mut_ptr() as *mut c_void, (name.len() * 2) as u32) } != 0 {
        let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        product = String::from_utf16_lossy(&name[..end]);
    }
    Some(Candidate { path: path.to_string(), product_id: attrs.ProductID, product })
}

/// An opened vendor HID collection that uses the 17-byte report protocol.
pub struct Device {
    handle: OwnedHandle,
    event: OwnedHandle,
    input_len: usize,
    output_len: usize,
    pub product_id: u16,
    pub product: String,
}

unsafe impl Send for Device {}

/// Open the Pulsar command collection that is actually answering. A receiver and a wired mouse
/// both expose the collection, but only the live link replies, so each is probed with a version read.
pub fn open() -> Result<Device, String> {
    let candidates = enumerate();
    if candidates.is_empty() {
        return Err("No Pulsar mouse is connected. Plug in the receiver or the mouse's cable.".into());
    }
    let mut opened = Vec::new();
    for candidate in &candidates {
        if let Ok(device) = open_path(candidate) {
            opened.push(device);
        }
    }
    if opened.is_empty() {
        return Err("Found a Pulsar device but not its control interface. Close Pulsar's own app and retry.".into());
    }
    // Prefer a collection that answers a version read; fall back to the first control interface.
    if let Some(index) = opened.iter().position(|d| d.version().is_some()) {
        return Ok(opened.swap_remove(index));
    }
    Ok(opened.swap_remove(0))
}

fn open_path(candidate: &Candidate) -> Result<Device, String> {
    let wide = wide(&candidate.path);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err("Couldn't open the Pulsar device. Close Pulsar's own app and try again.".into());
    }
    let handle = OwnedHandle(handle);
    let cap = caps(handle.0).ok_or("The Pulsar device has no HID capabilities.")?;
    let (input_len, output_len) = (cap.InputReportByteLength as usize, cap.OutputReportByteLength as usize);
    // The command collection is a 17-byte report on usage page 0xFF02; mouse/keyboard collections differ.
    if cap.UsagePage != COMMAND_USAGE_PAGE || input_len != REPORT_LEN || output_len != REPORT_LEN {
        return Err("Not the vendor collection.".into());
    }
    let event = unsafe { CreateEventW(null(), 1, 0, null()) };
    if event.is_null() {
        return Err("Couldn't create a wait event for the Pulsar device.".into());
    }
    Ok(Device {
        handle,
        event: OwnedHandle(event),
        input_len,
        output_len,
        product_id: candidate.product_id,
        product: candidate.product.clone(),
    })
}

fn report_lengths(handle: HANDLE) -> Option<(usize, usize)> {
    caps(handle).map(|c| (c.InputReportByteLength as usize, c.OutputReportByteLength as usize))
}

fn caps(handle: HANDLE) -> Option<HIDP_CAPS> {
    let mut preparsed: isize = 0;
    if unsafe { HidD_GetPreparsedData(handle, &mut preparsed) } == 0 {
        return None;
    }
    let mut caps: HIDP_CAPS = unsafe { std::mem::zeroed() };
    let status = unsafe { HidP_GetCaps(preparsed, &mut caps) };
    unsafe { HidD_FreePreparsedData(preparsed) };
    (status == HIDP_STATUS_SUCCESS).then_some(caps)
}

/// Build a 17-byte Pulsar report. Byte 0 is both the report id (0x08) and the first checksummed
/// byte; byte 1 is the command; bytes 3-4 are the big-endian address; byte 5 is the data length.
fn frame(cmd: u8, addr: u16, data: &[u8]) -> [u8; REPORT_LEN] {
    let mut f = [0u8; REPORT_LEN];
    f[0] = 0x08;
    f[1] = cmd;
    f[3] = (addr >> 8) as u8;
    f[4] = addr as u8;
    f[5] = data.len() as u8;
    f[6..6 + data.len().min(10)].copy_from_slice(&data[..data.len().min(10)]);
    f[16] = checksum(&f[..16]);
    f
}

impl Device {
    /// Send one 17-byte frame and return the first reply whose command byte matches and whose
    /// checksum verifies. `reply_cmd` matches Pulsar's own filter on reply byte 1.
    fn exchange(&self, frame: &[u8; REPORT_LEN], reply_cmd: u8, attempts: u32) -> Result<[u8; REPORT_LEN], String> {
        let deadline = Instant::now() + Duration::from_millis(600 * attempts.max(1) as u64);
        for _ in 0..attempts.max(1) {
            self.write(frame)?;
            loop {
                let reply = match self.read(Duration::from_millis(200)) {
                    Ok(Some(reply)) => reply,
                    Ok(None) => break,
                    Err(e) => return Err(e),
                };
                if reply[1] == reply_cmd && verify(&reply) {
                    return Ok(reply);
                }
                if Instant::now() > deadline {
                    break;
                }
            }
            if Instant::now() > deadline {
                break;
            }
        }
        Err("The Pulsar mouse didn't answer. It may be asleep; move it and try again.".into())
    }

    fn write(&self, report: &[u8]) -> Result<(), String> {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = self.event.0;
        unsafe { ResetEvent(self.event.0) };
        let ok = unsafe { WriteFile(self.handle.0, report.as_ptr(), report.len() as u32, null_mut(), &mut overlapped) };
        if ok == 0 {
            let pending = unsafe { windows_sys::Win32::Foundation::GetLastError() } == windows_sys::Win32::Foundation::ERROR_IO_PENDING;
            if !pending {
                return Err("Writing to the Pulsar device failed.".into());
            }
            if unsafe { WaitForSingleObject(self.event.0, 400) } != WAIT_OBJECT_0 {
                unsafe { CancelIo(self.handle.0) };
                return Err("Writing to the Pulsar device timed out.".into());
            }
        }
        let mut written = 0u32;
        unsafe { GetOverlappedResult(self.handle.0, &overlapped, &mut written, 0) };
        Ok(())
    }

    fn read(&self, timeout: Duration) -> Result<Option<[u8; REPORT_LEN]>, String> {
        let mut buf = vec![0u8; self.input_len];
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = self.event.0;
        unsafe { ResetEvent(self.event.0) };
        let ok = unsafe { ReadFile(self.handle.0, buf.as_mut_ptr(), buf.len() as u32, null_mut(), &mut overlapped) };
        if ok == 0 {
            let pending = unsafe { windows_sys::Win32::Foundation::GetLastError() } == windows_sys::Win32::Foundation::ERROR_IO_PENDING;
            if !pending {
                return Err("Reading from the Pulsar device failed.".into());
            }
            if unsafe { WaitForSingleObject(self.event.0, timeout.as_millis() as u32) } != WAIT_OBJECT_0 {
                unsafe { CancelIo(self.handle.0) };
                return Ok(None);
            }
        }
        let mut read = 0u32;
        if unsafe { GetOverlappedResult(self.handle.0, &overlapped, &mut read, 0) } == 0 || read < REPORT_LEN as u32 {
            return Ok(None);
        }
        let mut frame = [0u8; REPORT_LEN];
        frame.copy_from_slice(&buf[..REPORT_LEN]);
        Ok(Some(frame))
    }

    /// Read one flash value at `addr` with `len` bytes (<=10); returns the 10-byte data window.
    pub fn read_flash(&self, addr: u16, len: u8) -> Result<[u8; 10], String> {
        let request = frame(CMD_READ, addr, &[len.min(10)]);
        let reply = self.exchange(&request, CMD_READ, 3)?;
        let mut data = [0u8; 10];
        data.copy_from_slice(&reply[6..16]);
        Ok(data)
    }

    /// Write `value` (<=10 bytes, already including any per-value check byte) to flash at `addr`.
    /// Pulsar does not echo cmd 0x07, so this writes without waiting for a reply.
    pub fn write_flash(&self, addr: u16, value: &[u8]) -> Result<(), String> {
        if value.len() > 10 {
            return Err("Pulsar flash writes are limited to 10 bytes.".into());
        }
        let request = frame(CMD_WRITE, addr, value);
        self.write(&request)?;
        Ok(())
    }

    /// Current online state for the attached mouse (false when the wireless link is asleep).
    pub fn online(&self) -> bool {
        let request = frame(CMD_ONLINE, 0, &[]);
        self.exchange(&request, CMD_ONLINE, 2).map(|r| r[6] != 0).unwrap_or(false)
    }

    /// Battery percentage and charging flag, or `None` when the mouse doesn't answer.
    pub fn battery(&self) -> Option<(u8, bool)> {
        let request = frame(CMD_BATTERY, 0, &[]);
        let reply = self.exchange(&request, CMD_BATTERY, 3).ok()?;
        // reply[6] = percent, reply[7] = charging flag (UsbFinder_GetBatteryLevel).
        Some((reply[6].min(100), reply[7] != 0))
    }

    /// Firmware version word (receiver), high byte then low byte.
    pub fn version(&self) -> Option<u16> {
        let request = frame(CMD_VERSION, 0, &[]);
        let reply = self.exchange(&request, CMD_VERSION, 2).ok()?;
        Some(((reply[6] as u16) << 8) | reply[7] as u16)
    }
}

/// A reply is valid when its trailing check byte matches `0x55 - sum(first 16)`.
fn verify(reply: &[u8; REPORT_LEN]) -> bool {
    reply[16] == checksum(&reply[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_is_pulsar_complement() {
        // A GET-online frame checksums to 0x55 - (0x08 + 0x03) = 0x4a.
        let f = frame(CMD_ONLINE, 0, &[]);
        assert_eq!(f[16], 0x4a);
        assert!(verify(&f));
    }

    #[test]
    #[ignore]
    fn probe_device() {
        for c in enumerate() {
            println!("{:04x} {} {}", c.product_id, c.product, c.path);
        }
    }

    #[test]
    #[ignore]
    fn probe_collections() {
        for c in enumerate() {
            let wide = wide(&c.path);
            let h = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    null_mut(),
                )
            };
            if h == INVALID_HANDLE_VALUE {
                println!("{:04x} OPEN-FAIL {}", c.product_id, c.path);
                continue;
            }
            let h = OwnedHandle(h);
            let Some(cap) = caps(h.0) else { continue };
            if cap.InputReportByteLength as usize != REPORT_LEN {
                continue;
            }
            let col = c.path.split("col").nth(1).map(|s| s.chars().take(2).collect::<String>()).unwrap_or_default();
            print!(
                "pid={:04x} col{} usage={:02x}/{:04x} in={} out={} ",
                c.product_id, col, cap.Usage, cap.UsagePage, cap.InputReportByteLength, cap.OutputReportByteLength
            );
            let event = unsafe { CreateEventW(null(), 1, 0, null()) };
            let device = Device {
                handle: h,
                event: OwnedHandle(event),
                input_len: cap.InputReportByteLength as usize,
                output_len: cap.OutputReportByteLength as usize,
                product_id: c.product_id,
                product: c.product.clone(),
            };
            let request = frame(CMD_VERSION, 0, &[]);
            match device.exchange(&request, CMD_VERSION, 1) {
                Ok(r) => println!("VERSION OK {:02x?}", &r[..8]),
                Err(_) => println!("no reply"),
            }
        }
    }

    #[test]
    #[ignore]
    fn probe_protocol() {
        let device = open().expect("open");
        println!("pid {:04x} {}", device.product_id, device.product);
        println!("online {}", device.online());
        println!("battery {:?}", device.battery());
        println!("version {:?}", device.version());
        println!("dpi flash {:?}", device.read_flash(0x0008, 10));
    }

    #[test]
    #[ignore]
    fn dump_flash() {
        let device = open().expect("open");
        let mut bytes = Vec::new();
        for addr in 0u16..0x120 {
            match device.read_flash(addr, 1) {
                Ok(d) => bytes.push(d[0]),
                Err(_) => break,
            }
        }
        for (i, chunk) in bytes.chunks(16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            println!("{:#05x}: {}", i * 16, hex.join(" "));
        }
    }
}
