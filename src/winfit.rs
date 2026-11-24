//! Keep the active window of this program fully inside the work area of the monitor it is on.
//!
//! egui does not report where a monitor starts, so on Windows this asks the system directly.
//! Other platforms leave window placement to the window manager.

#[cfg(windows)]
pub fn fit_foreground() {
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };
    // SAFETY: plain Win32 calls on a window handle owned by the system; every out-parameter is a local.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != std::process::id() {
            return;
        }
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return;
        }
        let mut r: RECT = zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return;
        }
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let work = mi.rcWork;
        let x = r.left.min(work.right - w).max(work.left);
        let y = r.top.min(work.bottom - h).max(work.top);
        if x != r.left || y != r.top {
            SetWindowPos(hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

#[cfg(not(windows))]
pub fn fit_foreground() {}
