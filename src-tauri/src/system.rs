//! Windows integration: elevation and an elevated logon task for autostart.

use std::process::Command;

const TASK_NAME: &str = "Helm Fan Control";

#[cfg(windows)]
pub fn is_elevated() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Relaunches the current executable through the UAC prompt. Returns false if the user declined.
#[cfg(windows)]
pub fn relaunch_elevated(extra_args: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let Ok(exe) = std::env::current_exe() else { return false };
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let verb = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    let params = wide(extra_args.as_ref());

    let result = unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), std::ptr::null(), SW_SHOWNORMAL)
    };
    // ShellExecute returns a value > 32 on success.
    result as isize > 32
}

#[cfg(not(windows))]
pub fn relaunch_elevated(_extra_args: &str) -> bool {
    false
}

/// A scheduled task with "highest privileges" is the only way to autostart elevated without a UAC prompt.
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let mut command = Command::new("schtasks");
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        command.args([
            "/Create",
            "/F",
            "/TN",
            TASK_NAME,
            "/SC",
            "ONLOGON",
            "/RL",
            "HIGHEST",
            "/DELAY",
            "0000:15",
            "/TR",
            &format!("\"{}\" --autostart", exe.display()),
        ]);
    } else {
        command.args(["/Delete", "/F", "/TN", TASK_NAME]);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() || (!enabled && !autostart_exists()) {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

pub fn autostart_exists() -> bool {
    let mut command = Command::new("schtasks");
    command.args(["/Query", "/TN", TASK_NAME]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.output().map(|o| o.status.success()).unwrap_or(false)
}
