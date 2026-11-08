//! Disk health without vendor DLLs: IOCTL_STORAGE_QUERY_PROPERTY (descriptor, temperature, NVMe/ATA
//! protocol-specific queries) plus SMART_RCV_DRIVE_DATA for the classic ATA attribute table.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SmartAttribute {
    pub id: u8,
    pub name: &'static str,
    pub current: u8,
    pub worst: u8,
    pub threshold: u8,
    pub raw: u64,
    pub failing: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub letter: String,
    pub label: String,
    pub file_system: String,
    pub size: u64,
    pub free: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub index: u32,
    pub model: String,
    pub vendor: String,
    pub serial: String,
    pub firmware: String,
    pub bus: String,
    pub size: u64,
    pub temperature: Option<i32>,
    pub temperature_warning: Option<i32>,
    pub temperature_critical: Option<i32>,
    /// 0-100, derived the same way GCC's "Endurance" gauge is (100 - percentage used).
    pub health: Option<u8>,
    pub health_label: String,
    pub percentage_used: Option<u8>,
    pub available_spare: Option<u8>,
    pub spare_threshold: Option<u8>,
    pub critical_warning: Option<u8>,
    pub warnings: Vec<String>,
    pub power_on_hours: Option<u64>,
    pub power_cycles: Option<u64>,
    pub unsafe_shutdowns: Option<u64>,
    pub media_errors: Option<u64>,
    pub error_log_entries: Option<u64>,
    pub bytes_read: Option<f64>,
    pub bytes_written: Option<f64>,
    pub smart: Vec<SmartAttribute>,
    pub volumes: Vec<Volume>,
}

fn health_label(health: Option<u8>, warnings: &[String]) -> String {
    match health {
        _ if !warnings.is_empty() => "Caution".into(),
        Some(0) => "Bad".into(),
        Some(h) if h <= 10 => "Caution".into(),
        Some(_) => "Good".into(),
        None => "Unknown".into(),
    }
}

fn smart_name(id: u8) -> &'static str {
    match id {
        1 => "Raw read error rate",
        5 => "Reallocated sectors",
        9 => "Power-on hours",
        12 => "Power cycles",
        160 => "Uncorrectable sectors",
        161 => "Valid spare blocks",
        163 => "Initial invalid blocks",
        164 => "Total TLC erase count",
        165 => "Maximum erase count",
        166 => "Minimum erase count",
        167 => "Average erase count",
        168 => "Max NAND erase count (spec)",
        169 => "Remaining life",
        171 => "Program fail count",
        172 => "Erase fail count",
        173 => "Wear leveling count",
        174 => "Unexpected power loss",
        175 => "Power loss protection failure",
        177 => "Wear range delta",
        181 => "Program fail count (total)",
        182 => "Erase fail count (total)",
        183 => "SATA downshift errors",
        184 => "End-to-end errors",
        187 => "Reported uncorrectable errors",
        188 => "Command timeouts",
        190 => "Airflow temperature",
        192 => "Unsafe shutdowns",
        194 => "Temperature",
        195 => "ECC error rate",
        196 => "Reallocation events",
        197 => "Pending sectors",
        198 => "Offline uncorrectable",
        199 => "UDMA CRC errors",
        202 => "Percentage lifetime used",
        230 => "Drive life protection",
        231 => "SSD life left",
        232 => "Available reserved space",
        233 => "Media wearout indicator",
        234 => "NAND writes (vendor)",
        235 => "Good block count",
        241 => "Total host writes",
        242 => "Total host reads",
        244 => "Average erase count",
        245 => "Max erase count",
        246 => "Total host sectors written",
        _ => "Vendor specific",
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDiskFreeSpaceExW, GetLogicalDrives, GetVolumeInformationW, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    const IOCTL_STORAGE_GET_DEVICE_NUMBER: u32 = 0x002D_1080;
    const IOCTL_DISK_GET_DRIVE_GEOMETRY_EX: u32 = 0x0007_00A0;
    const SMART_RCV_DRIVE_DATA: u32 = 0x0007_C088;
    const PROTO_ATA: u32 = 2;
    const PROTO_NVME: u32 = 3;
    const DATA_LOG_PAGE: u32 = 2;
    const PROPERTY_DEVICE: u32 = 0;
    const PROPERTY_PROTOCOL_SPECIFIC: u32 = 50;
    const PROPERTY_TEMPERATURE: u32 = 52;

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open(path: &str, access: u32) -> Option<Handle> {
        let p = wide(path);
        let h = unsafe { CreateFileW(p.as_ptr(), access, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut()) };
        (h != INVALID_HANDLE_VALUE).then_some(Handle(h))
    }

    fn ioctl(h: &Handle, code: u32, input: &[u8], out: &mut [u8]) -> Option<usize> {
        let mut n = 0u32;
        let ok = unsafe {
            DeviceIoControl(h.0, code, input.as_ptr().cast(), input.len() as u32, out.as_mut_ptr().cast(), out.len() as u32, &mut n, null_mut())
        };
        (ok != 0).then_some(n as usize)
    }

    fn put(b: &mut [u8], o: usize, v: u32) {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn u16at(b: &[u8], o: usize) -> u16 {
        u16::from_le_bytes([b[o], b[o + 1]])
    }
    fn u32at(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
    }
    fn u64at(b: &[u8], o: usize) -> u64 {
        u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
    }
    fn u128at(b: &[u8], o: usize) -> u128 {
        u128::from_le_bytes(b[o..o + 16].try_into().unwrap())
    }
    fn cstr(b: &[u8]) -> String {
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf8_lossy(&b[..end]).trim().to_string()
    }
    fn sat(v: u128) -> u64 {
        v.min(u64::MAX as u128) as u64
    }

    fn bus_name(bus: u32) -> &'static str {
        match bus {
            1 => "SCSI",
            2 => "ATAPI",
            3 => "ATA",
            7 => "USB",
            8 => "RAID",
            10 => "SAS",
            11 => "SATA",
            12 => "SD",
            13 => "MMC",
            15 => "Virtual",
            17 => "NVMe",
            18 => "SCM",
            19 => "UFS",
            _ => "Other",
        }
    }

    fn describe(h: &Handle, disk: &mut Disk) -> Option<()> {
        let mut q = [0u8; 12];
        put(&mut q, 0, PROPERTY_DEVICE);
        let mut o = vec![0u8; 1024];
        ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &q, &mut o)?;
        let field = |off: usize| {
            let p = u32at(&o, off) as usize;
            if p == 0 || p >= o.len() { String::new() } else { cstr(&o[p..]) }
        };
        let tidy = |s: String| s.split_whitespace().collect::<Vec<_>>().join(" ");
        disk.vendor = tidy(field(12));
        disk.model = tidy(field(16));
        disk.firmware = tidy(field(20));
        disk.serial = field(24).trim_end_matches('.').to_string();
        disk.bus = bus_name(u32at(&o, 28)).to_string();
        if !disk.vendor.is_empty() && !disk.model.starts_with(&disk.vendor) {
            disk.model = format!("{} {}", disk.vendor, disk.model);
        }

        let mut g = [0u8; 256];
        if ioctl(h, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], &mut g).is_some() {
            disk.size = u64at(&g, 24);
        }
        Some(())
    }

    fn temperature(h: &Handle, disk: &mut Disk) {
        let mut q = [0u8; 12];
        put(&mut q, 0, PROPERTY_TEMPERATURE);
        let mut o = [0u8; 512];
        if ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &q, &mut o).is_none() || u16at(&o, 12) == 0 {
            return;
        }
        let t = i16::from_le_bytes([o[26], o[27]]) as i32;
        if t > -50 && t < 150 {
            disk.temperature = Some(t);
        }
        let crit = i16::from_le_bytes([o[8], o[9]]) as i32;
        let warn = i16::from_le_bytes([o[10], o[11]]) as i32;
        if crit > 0 {
            disk.temperature_critical = Some(crit);
        }
        if warn > 0 {
            disk.temperature_warning = Some(warn);
        }
    }

    fn protocol_query(h: &Handle, proto: u32, dtype: u32, req: u32, sub: u32, len: usize) -> Option<Vec<u8>> {
        let mut b = vec![0u8; 48 + len];
        put(&mut b, 0, PROPERTY_PROTOCOL_SPECIFIC);
        put(&mut b, 8, proto);
        put(&mut b, 12, dtype);
        put(&mut b, 16, req);
        put(&mut b, 20, sub);
        put(&mut b, 24, 40);
        put(&mut b, 28, len as u32);
        let input = b.clone();
        ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &input, &mut b)?;
        Some(b[48..48 + len].to_vec())
    }

    fn nvme_health(h: &Handle, disk: &mut Disk) -> bool {
        let Some(d) = protocol_query(h, PROTO_NVME, DATA_LOG_PAGE, 0x02, 0, 512) else { return false };
        let warning = d[0];
        disk.critical_warning = Some(warning);
        let kelvin = u16at(&d, 1) as i32;
        if disk.temperature.is_none() && kelvin > 200 {
            disk.temperature = Some(kelvin - 273);
        }
        disk.available_spare = Some(d[3]);
        disk.spare_threshold = Some(d[4]);
        disk.percentage_used = Some(d[5]);
        disk.health = Some(100u8.saturating_sub(d[5].min(100)));
        disk.bytes_read = Some(u128at(&d, 32) as f64 * 512_000.0);
        disk.bytes_written = Some(u128at(&d, 48) as f64 * 512_000.0);
        disk.power_cycles = Some(sat(u128at(&d, 112)));
        disk.power_on_hours = Some(sat(u128at(&d, 128)));
        disk.unsafe_shutdowns = Some(sat(u128at(&d, 144)));
        disk.media_errors = Some(sat(u128at(&d, 160)));
        disk.error_log_entries = Some(sat(u128at(&d, 176)));
        let flags = [
            (0x01, "Available spare is below threshold"),
            (0x02, "Temperature is outside the safe range"),
            (0x04, "Reliability degraded by media or internal errors"),
            (0x08, "Drive has switched to read-only mode"),
            (0x10, "Volatile memory backup has failed"),
        ];
        disk.warnings.extend(flags.iter().filter(|(bit, _)| warning & bit != 0).map(|(_, m)| m.to_string()));
        true
    }

    /// ATA Device Statistics log (0x04): a qword whose bit 63 marks it valid, value in bits 0-47.
    fn ata_devstat(h: &Handle, page: u32, off: usize) -> Option<u64> {
        let d = protocol_query(h, PROTO_ATA, DATA_LOG_PAGE, 0x04, page, 512)?;
        let q = u64at(&d, off);
        (q >> 63 == 1).then_some(q & 0xFFFF_FFFF_FFFF)
    }

    fn ata_stats(h: &Handle, disk: &mut Disk) {
        if let Some(poh) = ata_devstat(h, 1, 16) {
            disk.power_on_hours = Some(poh & 0xFFFF_FFFF);
        }
        if let Some(cycles) = ata_devstat(h, 1, 8) {
            disk.power_cycles = Some(cycles & 0xFFFF_FFFF);
        }
        if let Some(written) = ata_devstat(h, 1, 24) {
            disk.bytes_written = Some(written as f64 * 512.0);
        }
        if let Some(read) = ata_devstat(h, 1, 40) {
            disk.bytes_read = Some(read as f64 * 512.0);
        }
        if let Some(used) = ata_devstat(h, 7, 8) {
            let used = (used & 0xFF) as u8;
            disk.percentage_used = Some(used);
            disk.health = Some(100u8.saturating_sub(used.min(100)));
        }
        if disk.temperature.is_none() {
            if let Some(t) = ata_devstat(h, 5, 8) {
                let t = (t & 0xFF) as i8 as i32;
                if t > 0 && t < 120 {
                    disk.temperature = Some(t);
                }
            }
        }
    }

    fn smart_cmd(h: &Handle, feature: u8) -> Option<Vec<u8>> {
        let mut i = [0u8; 32];
        put(&mut i, 0, 512);
        i[4..11].copy_from_slice(&[feature, 1, 1, 0x4F, 0xC2, 0xA0, 0xB0]);
        let mut o = vec![0u8; 16 + 512];
        ioctl(h, SMART_RCV_DRIVE_DATA, &i, &mut o)?;
        Some(o[16..].to_vec())
    }

    fn ata_smart(index: u32, disk: &mut Disk) {
        // SMART_RCV_DRIVE_DATA needs a read/write handle, which requires administrator rights.
        let Some(h) = open(&format!(r"\\.\PhysicalDrive{index}"), GENERIC_READ | GENERIC_WRITE) else { return };
        let Some(data) = smart_cmd(&h, 0xD0) else { return };
        let thresholds = smart_cmd(&h, 0xD1).unwrap_or_else(|| vec![0; 512]);
        for k in 0..30 {
            let p = 2 + k * 12;
            let id = data[p];
            if id == 0 {
                continue;
            }
            let raw = (0..6).fold(0u64, |a, j| a | (data[p + 5 + j] as u64) << (8 * j));
            let threshold = (0..30).map(|m| 2 + m * 12).find(|&q| thresholds[q] == id).map_or(0, |q| thresholds[q + 1]);
            let current = data[p + 3];
            disk.smart.push(SmartAttribute {
                id,
                name: smart_name(id),
                current,
                worst: data[p + 4],
                threshold,
                raw,
                failing: threshold > 0 && current <= threshold,
            });
        }

        let find = |id: u8| disk.smart.iter().find(|a| a.id == id).cloned();
        if disk.temperature.is_none() {
            if let Some(t) = find(194).or_else(|| find(190)) {
                let c = (t.raw & 0xFF) as i32;
                if c > 0 && c < 120 {
                    disk.temperature = Some(c);
                }
            }
        }
        if disk.power_on_hours.is_none() {
            disk.power_on_hours = find(9).map(|a| a.raw & 0xFFFF_FFFF);
        }
        if disk.power_cycles.is_none() {
            disk.power_cycles = find(12).map(|a| a.raw);
        }
        if disk.unsafe_shutdowns.is_none() {
            disk.unsafe_shutdowns = find(192).or_else(|| find(174)).map(|a| a.raw);
        }
        if disk.health.is_none() {
            if let Some(life) = find(231).or_else(|| find(169)).or_else(|| find(233)) {
                disk.health = Some(life.current.min(100));
            }
        }
        for (id, what) in [(5u8, "reallocated sectors"), (197, "pending sectors"), (198, "uncorrectable sectors")] {
            if let Some(a) = find(id) {
                if a.raw > 0 {
                    disk.warnings.push(format!("{} {what}", a.raw));
                }
            }
        }
        if disk.smart.iter().any(|a| a.failing) {
            disk.warnings.push("A SMART attribute is at or below its failure threshold".into());
        }
    }

    fn volumes() -> Vec<(u32, Volume)> {
        let mask = unsafe { GetLogicalDrives() };
        let mut out = Vec::new();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = char::from(b'A' + i as u8);
            let Some(h) = open(&format!(r"\\.\{letter}:"), 0) else { continue };
            let mut num = [0u8; 12];
            if ioctl(&h, IOCTL_STORAGE_GET_DEVICE_NUMBER, &[], &mut num).is_none() {
                continue;
            }
            let disk = u32at(&num, 4);

            let root = wide(&format!("{letter}:\\"));
            let mut label = [0u16; 261];
            let mut fs = [0u16; 261];
            unsafe {
                GetVolumeInformationW(root.as_ptr(), label.as_mut_ptr(), 261, null_mut(), null_mut(), null_mut(), fs.as_mut_ptr(), 261);
            }
            let (mut free, mut total) = (0u64, 0u64);
            unsafe { GetDiskFreeSpaceExW(root.as_ptr(), null_mut(), &mut total, &mut free) };
            let s = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
            out.push((disk, Volume { letter: letter.to_string(), label: s(&label), file_system: s(&fs), size: total, free }));
        }
        out
    }

    pub fn scan() -> Vec<Disk> {
        let vols = volumes();
        let mut disks = Vec::new();
        let mut misses = 0;
        for index in 0..64u32 {
            let Some(h) = open(&format!(r"\\.\PhysicalDrive{index}"), 0) else {
                misses += 1;
                if misses > 8 {
                    break;
                }
                continue;
            };
            misses = 0;
            let mut disk = Disk { index, ..Default::default() };
            if describe(&h, &mut disk).is_none() {
                continue;
            }
            temperature(&h, &mut disk);
            let nvme = disk.bus == "NVMe" && nvme_health(&h, &mut disk);
            if !nvme && matches!(disk.bus.as_str(), "SATA" | "ATA") {
                ata_stats(&h, &mut disk);
                drop(h);
                ata_smart(index, &mut disk);
            }
            disk.health_label = health_label(disk.health, &disk.warnings);
            disk.volumes = vols.iter().filter(|(d, _)| *d == index).map(|(_, v)| v.clone()).collect();
            disks.push(disk);
        }
        disks
    }
}

#[cfg(windows)]
pub use win::scan;

#[cfg(not(windows))]
pub fn scan() -> Vec<Disk> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "reads the local disks"]
    fn scan_local_disks() {
        for d in super::scan() {
            println!("{}", serde_json::to_string(&d).unwrap());
        }
    }
}
