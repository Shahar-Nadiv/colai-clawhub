//! Claude Desktop, beside the toolbar.
//!
//! A mark handed to a chat open in the desktop app is answered there, where the conversation has
//! everything the toolbar's own card does not — artifacts, the diff pane, the whole thread. So the
//! toolbar's job after delivering it is to step aside and bring that window forward, not to keep a
//! copy of the reply in a popup of its own.
//!
//! The `claude` CLI has `--desktop --resume <id>`, but it refuses to run without a terminal and
//! refuses a session that is already open — which a desktop chat always is. Bringing the app's own
//! window forward is the dependable half, and the half that matters: the chat is already showing
//! in it.

/// Bring the Claude Desktop window to the front, restoring it if it was minimized.
#[tauri::command]
pub(crate) fn colai_open_desktop() -> Result<(), String> {
    bring_forward()
}

#[cfg(target_os = "windows")]
fn bring_forward() -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE, SW_SHOW,
    };

    /// Whether this pid is the desktop app — `Claude.exe` from its package, not the `claude`
    /// CLI, which has no window anyway but is named nearly the same.
    fn is_the_app(pid: u32) -> bool {
        let named = image_of(pid);
        if std::env::var_os("COLAI_DESKTOP_PROBE").is_some() {
            eprintln!("pid {pid}: {named:?}");
        }
        named.is_some_and(|file| file.eq_ignore_ascii_case("Claude.exe"))
    }

    fn image_of(pid: u32) -> Option<String> {
        // SAFETY: the handle is opened, read and closed here; the buffer is ours and its length
        // is passed in and written back.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut buffer = [0u16; 1024];
            let mut length = buffer.len() as u32;
            let read =
                QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut length) != 0;
            CloseHandle(process);
            if !read {
                return None;
            }
            let path = String::from_utf16_lossy(&buffer[..length as usize]);
            path.rsplit(['\\', '/']).next().map(str::to_string)
        }
    }

    unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
        // A top-level, unowned window with a title: the app's main window, not a tooltip or a
        // helper. Minimized still counts — restoring it is half the point.
        if !GetWindow(window, GW_OWNER).is_null() || GetWindowTextLengthW(window) == 0 {
            return 1;
        }
        if IsWindowVisible(window) == 0 && IsIconic(window) == 0 {
            return 1;
        }
        // The app is Electron; its main window is Chromium's. The `claude` CLI shares the file
        // name (`claude.exe`) but never owns a window of this class.
        let mut class = [0u16; 64];
        let written = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
        if String::from_utf16_lossy(&class[..written.max(0) as usize]) != "Chrome_WidgetWin_1" {
            return 1;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        if pid != 0 && is_the_app(pid) {
            *(found as *mut HWND) = window;
            return 0;
        }
        1
    }

    let mut window: HWND = std::ptr::null_mut();
    // SAFETY: `each` writes at most one HWND through the pointer it is given, which outlives the
    // enumeration; the window calls only read or change that window's own show state.
    unsafe {
        EnumWindows(Some(each), &mut window as *mut HWND as LPARAM);
        if window.is_null() {
            return Err("Claude Desktop isn't open.".to_string());
        }
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        } else {
            ShowWindow(window, SW_SHOW);
        }
        SetForegroundWindow(window);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn bring_forward() -> Result<(), String> {
    let opened = std::process::Command::new("open")
        .args(["-a", "Claude"])
        .status()
        .map_err(|trouble| format!("Could not open Claude Desktop: {trouble}"))?;
    if opened.success() {
        Ok(())
    } else {
        Err("Claude Desktop isn't installed.".to_string())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn bring_forward() -> Result<(), String> {
    Err("Claude Desktop isn't available on this platform.".to_string())
}

#[cfg(test)]
mod on_this_machine {
    /// Run by hand (`cargo test brings_the_app_forward -- --ignored`): it moves a real window.
    #[test]
    #[ignore]
    fn brings_the_app_forward() {
        super::bring_forward().expect("Claude Desktop should come forward");
    }
}
