//! The Win32 calls tao has no door for: the work area of a monitor, the DWM
//! border of the window and the lift to the front of the topmost band. Every
//! `unsafe` block of the crate is in this file, one call to each.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::mem::size_of;

use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
};

use crate::place::{PhysicalRect, rect_from_edges};

/// The handle tao gives a window (`hwnd()`), as the integer it comes as.
fn hwnd_of(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

fn rect_of(edges: RECT) -> Option<PhysicalRect> {
    rect_from_edges(edges.left, edges.top, edges.right, edges.bottom)
}

/// The work area of a monitor (its area less the taskbar) in physical px.
/// Meaningful only once the process is per-monitor DPI aware, which tao makes
/// it when the event loop is made. `None` when the system gives no area.
pub fn work_area(hmonitor: isize) -> Option<PhysicalRect> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    // SAFETY: `info` is a live MONITORINFO whose `cbSize` is set, as the call requires; the handle is one tao got from the system and a stale one makes the call return false.
    let found = unsafe { GetMonitorInfoW(HMONITOR(hmonitor as *mut c_void), &mut info) };
    found.as_bool().then(|| rect_of(info.rcWork)).flatten()
}

/// Asks DWM for no border and square corners on the window. `false` when
/// either is refused, which is what a Windows before build 22000 does.
pub fn hide_border(hwnd: isize) -> bool {
    let colour: u32 = DWMWA_COLOR_NONE;
    // SAFETY: the pointer is to a live u32 and the size passed is that of a u32, which is what DWMWA_BORDER_COLOR reads; a window handle that is gone makes the call fail with an error.
    let border = unsafe {
        DwmSetWindowAttribute(
            hwnd_of(hwnd),
            DWMWA_BORDER_COLOR,
            std::ptr::from_ref(&colour).cast(),
            size_of::<u32>() as u32,
        )
    };
    let corners = DWMWCP_DONOTROUND;
    // SAFETY: the pointer is to a live DWM_WINDOW_CORNER_PREFERENCE and the size passed is that of one, which is what DWMWA_WINDOW_CORNER_PREFERENCE reads.
    let rounding = unsafe {
        DwmSetWindowAttribute(
            hwnd_of(hwnd),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corners).cast(),
            size_of::<windows::Win32::Graphics::Dwm::DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        )
    };
    border.is_ok() && rounding.is_ok()
}

/// Lifts the window to the front of the topmost band without moving it,
/// resizing it or giving it the focus. The error is the system's code.
pub fn raise_topmost(hwnd: isize) -> Result<(), i32> {
    // SAFETY: the call takes the handle by value and no pointer; a handle that is gone makes it fail with an error, and the flags leave position, size and activation alone.
    let raised = unsafe {
        SetWindowPos(
            hwnd_of(hwnd),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
    raised.map_err(|error| error.code().0)
}

/// The build number of Windows, from the system itself and not from the
/// manifest the process carries. `None` when the call fails.
pub fn windows_build() -> Option<u32> {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..OSVERSIONINFOW::default()
    };
    // SAFETY: `info` is a live OSVERSIONINFOW whose size field is set, as RtlGetVersion requires.
    let status = unsafe { RtlGetVersion(&mut info) };
    status.is_ok().then_some(info.dwBuildNumber)
}
