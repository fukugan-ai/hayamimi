//! The Win32 side of the acceptance tests of the overlay: reading a window
//! back from the OS, plain windows to click and stack, a left click, and a
//! screen grab.
//!
//! The overlay is judged by what the OS says about its window, never by what
//! the overlay says about itself. Everything here goes through `HWND`s found
//! by class name and process id.
//!
//! Every `unsafe` block holds exactly one call, and the `SAFETY:` line above it
//! says why that call is sound. The process is made per-monitor-v2 DPI aware
//! once (`per_monitor_aware`) before any window is read, so every rectangle in
//! this file is in physical pixels.

#![allow(unsafe_code, dead_code)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW,
    HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetLastInputInfo, INPUT, INPUT_0, INPUT_MOUSE, LASTINPUTINFO, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, GA_ROOT,
    GWL_EXSTYLE, GWL_STYLE, GetAncestor, GetClassNameW, GetClientRect, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, HWND_TOPMOST, IsWindowVisible, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowPos, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_LBUTTONDOWN, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_OVERLAPPEDWINDOW,
    WS_SIZEBOX, WS_VISIBLE, WindowFromPoint,
};
use windows::core::{BOOL, w};

/// The extended-style bits the overlay is judged on (OV-1).
pub const EX_TOPMOST: u32 = WS_EX_TOPMOST.0;
pub const EX_LAYERED: u32 = WS_EX_LAYERED.0;
pub const EX_TRANSPARENT: u32 = WS_EX_TRANSPARENT.0;
pub const EX_TOOLWINDOW: u32 = WS_EX_TOOLWINDOW.0;
/// The style bit of a window whose edge can be grabbed to resize it.
pub const STYLE_SIZEBOX: u32 = WS_SIZEBOX.0;

/// The class of the plain windows the tests make themselves.
pub const TARGET_CLASS: &str = "hayamimi-ov-target";

/// A window handle that can cross threads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Win(pub isize);

impl Win {
    fn raw(self) -> HWND {
        HWND(self.0 as *mut c_void)
    }

    fn from_raw(handle: HWND) -> Win {
        Win(handle.0 as isize)
    }
}

/// A rectangle in physical pixels, edges as the OS reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn center(&self) -> (i32, i32) {
        (self.left + self.width() / 2, self.top + self.height() / 2)
    }

    pub fn inflate(&self, by: i32) -> Rect {
        Rect {
            left: self.left - by,
            top: self.top - by,
            right: self.right + by,
            bottom: self.bottom + by,
        }
    }

    fn from_raw(raw: RECT) -> Rect {
        Rect {
            left: raw.left,
            top: raw.top,
            right: raw.right,
            bottom: raw.bottom,
        }
    }
}

/// What the OS says about one window.
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    pub ex_style: u32,
    pub style: u32,
    pub window: Rect,
    /// Width and height of the client area.
    pub client: (i32, i32),
    pub visible: bool,
}

impl Facts {
    /// Whether the client area is the whole window: no frame, no title bar.
    pub fn frameless(&self) -> bool {
        self.client == (self.window.width(), self.window.height())
    }
}

static AWARE: OnceLock<Result<(), String>> = OnceLock::new();

/// Makes this process per-monitor-v2 DPI aware, once. Call it before reading
/// any rectangle: a process that is not aware reads divided values.
pub fn per_monitor_aware() -> Result<(), String> {
    AWARE
        .get_or_init(|| {
            // SAFETY: takes a constant context value and changes only this process's DPI awareness; no window of this process exists yet.
            let set = unsafe {
                SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            };
            set.map_err(|error| format!("SetProcessDpiAwarenessContext: {error}"))
        })
        .clone()
}

extern "system" fn collect_window(handle: HWND, found: LPARAM) -> BOOL {
    // SAFETY: `found` is the address of the `Vec<isize>` that `z_order` keeps alive for the whole synchronous EnumWindows call, and nothing else touches it meanwhile.
    let list = unsafe { &mut *(found.0 as *mut Vec<isize>) };
    list.push(handle.0 as isize);
    BOOL(1)
}

/// The top-level windows, topmost first (the order `EnumWindows` gives).
pub fn z_order() -> Result<Vec<Win>, String> {
    let mut found: Vec<isize> = Vec::new();
    let address = LPARAM(std::ptr::from_mut(&mut found) as isize);
    // SAFETY: the callback only pushes into `found`, whose address is passed as `address` and which outlives this synchronous call.
    let walked = unsafe { EnumWindows(Some(collect_window), address) };
    walked.map_err(|error| format!("EnumWindows: {error}"))?;
    Ok(found.into_iter().map(Win).collect())
}

/// The window class of `w`, when the OS answers.
pub fn class_name(w: Win) -> Option<String> {
    let mut buffer = [0_u16; 256];
    // SAFETY: `buffer` is a live array the call may fill; a dead handle makes the call return 0.
    let length = unsafe { GetClassNameW(w.raw(), &mut buffer) };
    let count = usize::try_from(length).ok().filter(|count| *count > 0)?;
    buffer.get(..count).map(String::from_utf16_lossy)
}

/// The process that owns `w`, when the OS answers.
pub fn window_pid(w: Win) -> Option<u32> {
    let mut pid = 0_u32;
    // SAFETY: `pid` is a live u32 the call writes; a dead handle leaves it 0.
    let thread = unsafe { GetWindowThreadProcessId(w.raw(), Some(&mut pid)) };
    (thread != 0 && pid != 0).then_some(pid)
}

/// The top-level windows of `class` that process `pid` owns.
pub fn windows_of(class: &str, pid: u32) -> Result<Vec<Win>, String> {
    Ok(z_order()?
        .into_iter()
        .filter(|w| class_name(*w).is_some_and(|name| name == class))
        .filter(|w| window_pid(*w) == Some(pid))
        .collect())
}

/// Whether any top-level window of `class` exists, whoever owns it.
pub fn class_exists(class: &str) -> Result<bool, String> {
    Ok(z_order()?
        .into_iter()
        .any(|w| class_name(w).is_some_and(|name| name == class)))
}

fn long_ptr(
    w: Win,
    index: windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX,
) -> Result<u32, String> {
    // SAFETY: only reads the window's style word; a dead handle makes the call return 0.
    let raw = unsafe { GetWindowLongPtrW(w.raw(), index) };
    u32::try_from(raw & 0xFFFF_FFFF).map_err(|error| format!("style word: {error}"))
}

pub fn window_rect(w: Win) -> Result<Rect, String> {
    let mut raw = RECT::default();
    // SAFETY: `raw` is a live RECT the call writes.
    let read = unsafe { GetWindowRect(w.raw(), &mut raw) };
    read.map_err(|error| format!("GetWindowRect: {error}"))?;
    Ok(Rect::from_raw(raw))
}

fn client_size(w: Win) -> Result<(i32, i32), String> {
    let mut raw = RECT::default();
    // SAFETY: `raw` is a live RECT the call writes.
    let read = unsafe { GetClientRect(w.raw(), &mut raw) };
    read.map_err(|error| format!("GetClientRect: {error}"))?;
    let client = Rect::from_raw(raw);
    Ok((client.width(), client.height()))
}

pub fn is_visible(w: Win) -> bool {
    // SAFETY: only reads the visibility bit; a dead handle makes the call return FALSE.
    unsafe { IsWindowVisible(w.raw()) }.as_bool()
}

/// The style words, rectangles and visibility of `w`, read from the OS.
pub fn facts(w: Win) -> Result<Facts, String> {
    Ok(Facts {
        ex_style: long_ptr(w, GWL_EXSTYLE)?,
        style: long_ptr(w, GWL_STYLE)?,
        window: window_rect(w)?,
        client: client_size(w)?,
        visible: is_visible(w),
    })
}

/// The DPI of the monitor `w` is on, as a ratio to 96.
pub fn scale_of(w: Win) -> Result<f64, String> {
    // SAFETY: only reads the DPI of the window; a dead handle makes the call return 0.
    let dpi = unsafe { GetDpiForWindow(w.raw()) };
    scale_from_dpi(dpi)
}

/// The DPI of the primary monitor, as a ratio to 96 (this process is per-monitor aware).
pub fn system_scale() -> Result<f64, String> {
    // SAFETY: takes no argument and only reads the system DPI.
    let dpi = unsafe { GetDpiForSystem() };
    scale_from_dpi(dpi)
}

fn scale_from_dpi(dpi: u32) -> Result<f64, String> {
    if dpi == 0 {
        return Err(String::from("the OS answered a DPI of 0"));
    }
    Ok(f64::from(dpi) / 96.0)
}

/// The whole primary monitor and its work area (the monitor without the taskbar).
pub fn primary_monitor() -> Result<(Rect, Rect), String> {
    let origin = POINT { x: 0, y: 0 };
    // SAFETY: takes the point by value; DEFAULTTOPRIMARY always yields a monitor.
    let monitor = unsafe { MonitorFromPoint(origin, MONITOR_DEFAULTTOPRIMARY) };
    let size = u32::try_from(size_of::<MONITORINFO>()).map_err(|error| error.to_string())?;
    let mut info = MONITORINFO {
        cbSize: size,
        ..Default::default()
    };
    // SAFETY: `info` is a live MONITORINFO with `cbSize` set; `monitor` was just returned by the OS.
    let read = unsafe { GetMonitorInfoW(monitor, &mut info) };
    if !read.as_bool() {
        return Err(String::from(
            "GetMonitorInfoW failed for the primary monitor",
        ));
    }
    Ok((Rect::from_raw(info.rcMonitor), Rect::from_raw(info.rcWork)))
}

pub fn foreground() -> Option<Win> {
    // SAFETY: takes no argument and only reads which window is in front.
    let front = unsafe { GetForegroundWindow() };
    (!front.is_invalid()).then(|| Win::from_raw(front))
}

/// Asks the OS to put `w` in front. False when the OS refuses (its foreground lock).
pub fn set_foreground(w: Win) -> bool {
    // SAFETY: a refusal returns FALSE and changes nothing; the handle is one this process made.
    unsafe { SetForegroundWindow(w.raw()) }.as_bool()
}

/// Shows `w` (`SW_SHOW`). A window this process made.
pub fn show(w: Win) {
    // SAFETY: only changes the visibility of a window this process made.
    let was_visible = unsafe { ShowWindow(w.raw(), SW_SHOW) };
    // The previous visibility says nothing the tests need.
    let _ = was_visible;
}

/// Raises `w` to the top of its own band (topmost or not) without changing the band.
pub fn bring_to_top(w: Win) -> Result<(), String> {
    // SAFETY: only changes the z-order of a window this process made.
    let raised = unsafe { BringWindowToTop(w.raw()) };
    raised.map_err(|error| format!("BringWindowToTop: {error}"))
}

/// Puts `w` into the topmost band, above every topmost window.
pub fn make_topmost(w: Win) -> Result<(), String> {
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    // SAFETY: changes only the z-order of a window this process made; position and size are left alone by the flags.
    let moved = unsafe { SetWindowPos(w.raw(), Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
    moved.map_err(|error| format!("SetWindowPos: {error}"))
}

/// Posts `WM_CLOSE` to `w`.
pub fn post_close(w: Win) -> Result<(), String> {
    // SAFETY: posting a message to a window handle is sound for any value; a dead handle makes the call fail.
    let posted = unsafe { PostMessageW(Some(w.raw()), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    posted.map_err(|error| format!("PostMessageW: {error}"))
}

// ---- a left click ------------------------------------------------------

fn mouse_input(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    let size = i32::try_from(size_of::<INPUT>()).map_err(|error| error.to_string())?;
    // SAFETY: `inputs` is a live slice of fully initialised INPUT records and `size` is size_of::<INPUT>().
    let sent = unsafe { SendInput(inputs, size) };
    if usize::try_from(sent).is_ok_and(|count| count == inputs.len()) {
        return Ok(());
    }
    Err(format!(
        "SendInput placed {sent} of {} events",
        inputs.len()
    ))
}

fn metric(index: windows::Win32::UI::WindowsAndMessaging::SYSTEM_METRICS_INDEX) -> i32 {
    // SAFETY: only reads a system metric.
    unsafe { GetSystemMetrics(index) }
}

/// Maps a physical pixel to the 0..=65535 range `SendInput` takes for an
/// absolute position on the virtual desktop whose edge starts at `origin` and
/// is `extent` pixels long.
pub fn normalise(pixel: i32, origin: i32, extent: i32) -> Result<i32, String> {
    if extent < 2 || pixel < origin || pixel >= origin + extent {
        return Err(format!(
            "pixel {pixel} is outside {origin}..{}",
            origin + extent
        ));
    }
    let scaled = i64::from(pixel - origin) * 65535 / i64::from(extent - 1);
    i32::try_from(scaled).map_err(|error| error.to_string())
}

/// The top-level window under the physical pixel `(x, y)`, if there is one.
/// Hidden and disabled windows are not found (`WindowFromPoint`); a child
/// window is reported as the top-level window that holds it.
pub fn window_at(x: i32, y: i32) -> Option<Win> {
    // SAFETY: takes the point by value and only reads which window is there.
    let found = unsafe { WindowFromPoint(POINT { x, y }) };
    if found.is_invalid() {
        return None;
    }
    // SAFETY: `found` was just returned by the OS; a window that has gone since makes the call return NULL.
    let top = unsafe { GetAncestor(found, GA_ROOT) };
    (!top.is_invalid()).then(|| Win::from_raw(top))
}

/// Whether the window `found` under a point is one of `own`. Pure, so that the
/// decision is tested without a window.
fn judge_target(found: Option<Win>, own: &[Win]) -> Result<(), String> {
    match found {
        Some(top) if own.contains(&top) => Ok(()),
        Some(top) => Err(format!(
            "the window there is {top:?} (class {:?}), not one of the test's own windows {own:?}",
            class_name(top)
        )),
        None => Err(format!(
            "no window is there; the test's own windows are {own:?}"
        )),
    }
}

/// Stops with an error unless the window under the physical pixel `(x, y)` is
/// one of `own`, the test's own windows. Nothing is sent.
pub fn guard_click(x: i32, y: i32, own: &[Win]) -> Result<(), String> {
    judge_target(window_at(x, y), own)
        .map_err(|why| format!("no input was sent to ({x}, {y}): {why}"))
}

/// The least idle time, in seconds, with which the first click may be sent:
/// the user is away.
pub const MIN_IDLE_SECS: u32 = 300;

/// What the test itself last did: where the pointer stood after its click, and
/// the tick of the last input the system saw right after it. A click that finds
/// the pointer elsewhere, or a later input tick, was preceded by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Trace {
    pointer: (i32, i32),
    input_tick: u32,
}

/// Whether a click may be sent now. Pure, so that each way of stopping is
/// tested without sending anything. The first click needs the user to have
/// been away `MIN_IDLE_SECS`; a later one needs the pointer where the test left
/// it and no input after the test's own (the test's input moves the idle clock
/// too, so the clock is compared to the tick the test saw, not to zero).
fn judge_before_click(
    trace: Option<Trace>,
    idle_secs: u32,
    input_tick: u32,
    cursor: (i32, i32),
) -> Result<(), String> {
    let Some(last) = trace else {
        if idle_secs < MIN_IDLE_SECS {
            return Err(format!(
                "no input was sent: the user was active {idle_secs} s ago, less than {MIN_IDLE_SECS} s"
            ));
        }
        return Ok(());
    };
    if cursor != last.pointer {
        return Err(format!(
            "no input was sent: the pointer is at {cursor:?}, not where the test left it, {:?}",
            last.pointer
        ));
    }
    if input_tick != last.input_tick {
        return Err(String::from(
            "no input was sent: the system saw an input after the test's own",
        ));
    }
    Ok(())
}

fn cursor_position() -> Result<(i32, i32), String> {
    let mut at = POINT::default();
    // SAFETY: `at` is a live POINT that the call fills in.
    unsafe { GetCursorPos(&mut at) }.map_err(|error| format!("GetCursorPos: {error}"))?;
    Ok((at.x, at.y))
}

/// The tick of the last input the system saw, and how many seconds ago that was.
fn last_input() -> Result<(u32, u32), String> {
    let size = u32::try_from(size_of::<LASTINPUTINFO>()).map_err(|error| error.to_string())?;
    let mut info = LASTINPUTINFO {
        cbSize: size,
        dwTime: 0,
    };
    // SAFETY: `info` is a live LASTINPUTINFO whose `cbSize` is its size.
    let read = unsafe { GetLastInputInfo(&mut info) };
    if !read.as_bool() {
        return Err(String::from("GetLastInputInfo failed"));
    }
    // SAFETY: takes no argument and only reads the tick count.
    let now = unsafe { GetTickCount() };
    Ok((now.wrapping_sub(info.dwTime) / 1000, info.dwTime))
}

/// The test's one source of mouse input. It stops, sending nothing, when the
/// user has been active (see `judge_before_click`) or when the window under
/// the pixel is not one of the test's own.
#[derive(Default)]
pub struct Pointer {
    trace: Option<Trace>,
}

impl Pointer {
    pub fn new() -> Pointer {
        Pointer::default()
    }

    fn check(&self) -> Result<(), String> {
        let (idle_secs, tick) = last_input()?;
        judge_before_click(self.trace, idle_secs, tick, cursor_position()?)
    }

    /// Moves the pointer to the physical pixel `(x, y)` and presses and
    /// releases the left button, in one `SendInput`, but only while the user is
    /// away and the window under that pixel is one of `own`.
    pub fn click(&mut self, x: i32, y: i32, own: &[Win]) -> Result<(), String> {
        self.check()?;
        guard_click(x, y, own)?;
        let across = normalise(x, metric(SM_XVIRTUALSCREEN), metric(SM_CXVIRTUALSCREEN))?;
        let down = normalise(y, metric(SM_YVIRTUALSCREEN), metric(SM_CYVIRTUALSCREEN))?;
        let place = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        send(&[
            mouse_input(across, down, place),
            mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN),
            mouse_input(0, 0, MOUSEEVENTF_LEFTUP),
        ])?;
        let (_, input_tick) = last_input()?;
        self.trace = Some(Trace {
            pointer: cursor_position()?,
            input_tick,
        });
        Ok(())
    }
}

/// Remembers the window that was in front and puts it back when dropped, also
/// when the test fails.
pub struct ForegroundKeeper<F: FnMut(Win)> {
    original: Option<Win>,
    restore: F,
}

impl<F: FnMut(Win)> ForegroundKeeper<F> {
    pub fn with(original: Option<Win>, restore: F) -> ForegroundKeeper<F> {
        ForegroundKeeper { original, restore }
    }
}

impl<F: FnMut(Win)> Drop for ForegroundKeeper<F> {
    fn drop(&mut self) {
        if let Some(original) = self.original {
            (self.restore)(original);
        }
    }
}

/// The keeper of the real foreground: the window in front now goes back in front when it is dropped.
pub fn keep_foreground() -> ForegroundKeeper<fn(Win)> {
    fn put_back(w: Win) {
        // A refusal leaves the front as it is; a drop cannot report it.
        let _ = set_foreground(w);
    }
    ForegroundKeeper::with(foreground(), put_back)
}

// ---- a screen grab -----------------------------------------------------

/// Pixels of a screen region, rows top to bottom, four bytes (B, G, R, unused) each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub bgra: Vec<u8>,
}

struct ScreenDc(HDC);

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: the DC came from GetDC(None) and is released exactly once, here.
        let released = unsafe { ReleaseDC(None, self.0) };
        // A drop has no way to report a failed release.
        let _ = released;
    }
}

struct MemoryDc(HDC);

impl Drop for MemoryDc {
    fn drop(&mut self) {
        // SAFETY: the DC came from CreateCompatibleDC and is deleted exactly once, here.
        let deleted = unsafe { DeleteDC(self.0) };
        // A drop has no way to report a failed delete.
        let _ = deleted;
    }
}

struct Bitmap(HBITMAP);

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: the bitmap came from CreateCompatibleBitmap, is selected into no DC by now, and is deleted exactly once, here.
        let deleted = unsafe { DeleteObject(HGDIOBJ(self.0.0)) };
        // A drop has no way to report a failed delete.
        let _ = deleted;
    }
}

/// Copies the screen region `r` (physical pixels), layered windows included.
pub fn capture(r: Rect) -> Result<Pixels, String> {
    let (width, height) = (r.width(), r.height());
    let columns = usize::try_from(width).map_err(|error| error.to_string())?;
    let rows = usize::try_from(height).map_err(|error| error.to_string())?;
    let lines = u32::try_from(height).map_err(|error| error.to_string())?;
    // SAFETY: None selects the whole screen; ScreenDc::drop releases it.
    let screen = ScreenDc(unsafe { GetDC(None) });
    if screen.0.is_invalid() {
        return Err(String::from("GetDC(None) failed"));
    }
    // SAFETY: `screen.0` is a live screen DC; MemoryDc::drop deletes the new one.
    let memory = MemoryDc(unsafe { CreateCompatibleDC(Some(screen.0)) });
    // SAFETY: `screen.0` is a live screen DC; Bitmap::drop deletes the bitmap.
    let bitmap = Bitmap(unsafe { CreateCompatibleBitmap(screen.0, width, height) });
    if memory.0.is_invalid() || bitmap.0.is_invalid() {
        return Err(String::from(
            "could not make a bitmap to copy the screen into",
        ));
    }
    // SAFETY: both handles are live and compatible; the previous object is put back below.
    let previous = unsafe { SelectObject(memory.0, HGDIOBJ(bitmap.0.0)) };
    // SAFETY: the memory DC holds the bitmap; the screen DC is live; the region is read-only on the screen side.
    let copied = unsafe {
        BitBlt(
            memory.0,
            0,
            0,
            width,
            height,
            Some(screen.0),
            r.left,
            r.top,
            SRCCOPY | CAPTUREBLT,
        )
    };
    // SAFETY: puts back the object the DC held before, so the bitmap is selected into no DC when it is read.
    unsafe { SelectObject(memory.0, previous) };
    copied.map_err(|error| format!("BitBlt: {error}"))?;
    read_bits(&memory, &bitmap, columns, rows, lines)
}

fn read_bits(
    memory: &MemoryDc,
    bitmap: &Bitmap,
    columns: usize,
    rows: usize,
    lines: u32,
) -> Result<Pixels, String> {
    let header_size = u32::try_from(size_of::<BITMAPINFOHEADER>()).map_err(|e| e.to_string())?;
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: header_size,
            biWidth: i32::try_from(columns).map_err(|e| e.to_string())?,
            biHeight: -i32::try_from(rows).map_err(|e| e.to_string())?,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0_u8; columns * rows * 4];
    // SAFETY: `bgra` holds columns * rows * 4 bytes, the size a 32-bit top-down read of `lines` rows needs; `info` is a live BITMAPINFO; the bitmap is selected into no DC.
    let copied = unsafe {
        GetDIBits(
            memory.0,
            bitmap.0,
            0,
            lines,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if usize::try_from(copied).is_ok_and(|count| count == rows) {
        return Ok(Pixels {
            width: columns,
            height: rows,
            bgra,
        });
    }
    Err(format!("GetDIBits copied {copied} of {rows} rows"))
}

// ---- a plain window the test makes itself ------------------------------

thread_local! {
    /// The click counter of the window made on this thread.
    static CLICKS: RefCell<Option<Arc<AtomicU32>>> = const { RefCell::new(None) };
}

extern "system" fn naive_proc(
    handle: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_LBUTTONDOWN {
        CLICKS.with_borrow(|counter| {
            if let Some(counter) = counter {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
    }
    if message == WM_DESTROY {
        // SAFETY: ends the message loop of the thread that runs this procedure; takes only an exit code.
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    // SAFETY: the arguments are exactly those the system handed to this procedure.
    unsafe { DefWindowProcW(handle, message, wparam, lparam) }
}

static NAIVE_CLASS: OnceLock<Result<(), String>> = OnceLock::new();

fn register_naive_class() -> Result<(), String> {
    NAIVE_CLASS
        .get_or_init(|| {
            // SAFETY: a null module name selects this executable's own module handle.
            let module = unsafe { GetModuleHandleW(None) }
                .map_err(|error| format!("GetModuleHandleW: {error}"))?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(naive_proc),
                hInstance: module.into(),
                lpszClassName: w!("hayamimi-ov-target"),
                ..Default::default()
            };
            // SAFETY: `class` is fully initialised and its name is a static wide literal.
            let atom = unsafe { RegisterClassW(&class) };
            if atom == 0 {
                return Err(String::from("RegisterClassW failed for the target class"));
            }
            Ok(())
        })
        .clone()
}

fn create_naive(at: Rect) -> Result<Win, String> {
    register_naive_class()?;
    // SAFETY: a null module name selects this executable's own module handle.
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|error| format!("GetModuleHandleW: {error}"))?;
    // SAFETY: the class was registered above; every pointer argument is a static literal or None; the window belongs to the calling thread, which runs its message loop.
    let made = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("hayamimi-ov-target"),
            w!("hayamimi-ov-target"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            at.left,
            at.top,
            at.width(),
            at.height(),
            None,
            None,
            Some(module.into()),
            None,
        )
    };
    made.map(Win::from_raw)
        .map_err(|error| format!("CreateWindowExW: {error}"))
}

fn message_loop() {
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is a live MSG; a null window handle takes the messages of every window of this thread.
        let got = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if got.0 <= 0 {
            return;
        }
        // SAFETY: `message` was just filled by GetMessageW.
        let translated = unsafe { TranslateMessage(&message) };
        // Whether a character message was posted does not matter to a window that reads none.
        let _ = translated;
        // SAFETY: `message` was just filled by GetMessageW and its window belongs to this thread.
        unsafe { DispatchMessageW(&message) };
    }
}

fn run_naive(at: Rect, clicks: Arc<AtomicU32>, ready: mpsc::Sender<Result<Win, String>>) {
    CLICKS.set(Some(clicks));
    let made = create_naive(at);
    let alive = made.is_ok();
    // The receiver is gone only when `open` already gave up, and then nothing is left to tell.
    let _ = ready.send(made);
    if alive {
        message_loop();
    }
}

/// A plain Win32 window made by the test, on its own thread with its own
/// message loop, that counts the left clicks it receives. Dropping it closes it.
pub struct Naive {
    win: Win,
    clicks: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl Naive {
    /// Opens a titled, resizable window (`WS_OVERLAPPEDWINDOW`) at `at`.
    pub fn open(at: Rect) -> Result<Naive, String> {
        per_monitor_aware()?;
        let clicks = Arc::new(AtomicU32::new(0));
        let (ready, born) = mpsc::channel();
        let counter = Arc::clone(&clicks);
        let thread = std::thread::spawn(move || run_naive(at, counter, ready));
        let win = born
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("the target window did not report back: {error}"))??;
        Ok(Naive {
            win,
            clicks,
            thread: Some(thread),
        })
    }

    pub fn win(&self) -> Win {
        self.win
    }

    /// The left clicks received so far.
    pub fn clicks(&self) -> u32 {
        self.clicks.load(Ordering::SeqCst)
    }
}

impl Drop for Naive {
    fn drop(&mut self) {
        // A window that is already gone cannot be asked again, and the join below still ends the thread.
        let _ = post_close(self.win);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Trace = Trace {
        pointer: (500, 400),
        input_tick: 9_000,
    };

    /// The first click needs the user away for `MIN_IDLE_SECS`, to the second.
    #[test]
    fn the_first_click_needs_the_user_to_have_been_away_300_seconds() {
        assert!(judge_before_click(None, MIN_IDLE_SECS - 1, 1, (0, 0)).is_err());
        assert!(judge_before_click(None, MIN_IDLE_SECS, 1, (0, 0)).is_ok());
    }

    /// A later click is not held to the idle clock: the test's own input has reset it.
    #[test]
    fn a_later_click_ignores_the_idle_clock_the_tests_own_input_reset() {
        assert!(judge_before_click(Some(LEFT), 0, LEFT.input_tick, LEFT.pointer).is_ok());
    }

    #[test]
    fn a_pointer_that_was_moved_since_the_last_click_stops_the_next_one() {
        let moved = judge_before_click(Some(LEFT), 0, LEFT.input_tick, (501, 400));
        let said = moved.err();
        assert!(
            said.as_deref().is_some_and(|text| text.contains("pointer")),
            "{said:?}"
        );
    }

    #[test]
    fn an_input_after_the_tests_own_stops_the_next_click() {
        let later = judge_before_click(Some(LEFT), 0, LEFT.input_tick + 1, LEFT.pointer);
        let said = later.err();
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("after the test's own")),
            "{said:?}"
        );
    }

    /// The real reads work: the cursor and the input clock can be read, and the idle time is not absurd.
    #[test]
    fn the_cursor_and_the_input_clock_can_be_read() -> Result<(), String> {
        cursor_position()?;
        let (idle_secs, _) = last_input()?;
        assert!(idle_secs < u32::MAX / 2000, "{idle_secs}");
        Ok(())
    }

    #[test]
    fn a_pointer_standing_where_the_test_expects_it_passes_the_real_read() -> Result<(), String> {
        let here = cursor_position()?;
        let (idle_secs, tick) = last_input()?;
        let kept = Trace {
            pointer: here,
            input_tick: tick,
        };
        let mut other = kept;
        other.pointer = (here.0 + 50, here.1);
        assert!(judge_before_click(Some(kept), idle_secs, tick, cursor_position()?).is_ok());
        assert!(judge_before_click(Some(other), idle_secs, tick, cursor_position()?).is_err());
        Ok(())
    }

    #[test]
    fn the_window_that_was_in_front_is_put_back_when_the_keeper_is_dropped() {
        let mut put_back = Vec::new();
        {
            let _keeper = ForegroundKeeper::with(Some(Win(7)), |w| put_back.push(w));
        }
        assert_eq!(put_back, [Win(7)]);
    }

    #[test]
    fn the_window_that_was_in_front_is_put_back_also_when_the_test_panics() {
        let mut put_back = Vec::new();
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _keeper = ForegroundKeeper::with(Some(Win(9)), |w| put_back.push(w));
            panic!("the test failed");
        }));
        assert!(ran.is_err());
        assert_eq!(put_back, [Win(9)]);
    }

    #[test]
    fn a_keeper_with_no_window_in_front_puts_nothing_back() {
        let mut put_back = Vec::new();
        drop(ForegroundKeeper::with(None, |w| put_back.push(w)));
        assert!(put_back.is_empty());
    }

    #[test]
    fn the_first_pixel_maps_to_zero_and_the_last_to_the_top_of_the_range() -> Result<(), String> {
        assert_eq!(normalise(0, 0, 3840)?, 0);
        assert_eq!(normalise(3839, 0, 3840)?, 65535);
        Ok(())
    }

    #[test]
    fn a_pixel_left_of_the_virtual_desktop_is_refused() {
        assert!(normalise(-1, 0, 3840).is_err());
        assert!(normalise(3840, 0, 3840).is_err());
        assert!(normalise(0, 0, 1).is_err());
    }

    #[test]
    fn a_desktop_that_starts_left_of_zero_maps_from_its_own_edge() -> Result<(), String> {
        assert_eq!(normalise(-1920, -1920, 5760)?, 0);
        assert_eq!(normalise(3839, -1920, 5760)?, 65535);
        Ok(())
    }

    #[test]
    fn a_rectangle_reports_its_size_centre_and_growth() {
        let rect = Rect {
            left: 10,
            top: 20,
            right: 110,
            bottom: 60,
        };
        assert_eq!((rect.width(), rect.height()), (100, 40));
        assert_eq!(rect.center(), (60, 40));
        let grown = rect.inflate(8);
        assert_eq!(
            (grown.left, grown.top, grown.right, grown.bottom),
            (2, 12, 118, 68)
        );
    }

    #[test]
    fn a_window_whose_client_area_is_its_whole_rectangle_is_frameless() {
        let window = Rect {
            left: 0,
            top: 0,
            right: 300,
            bottom: 100,
        };
        let bare = Facts {
            ex_style: 0,
            style: 0,
            window,
            client: (300, 100),
            visible: true,
        };
        let framed = Facts {
            client: (284, 61),
            ..bare
        };
        assert!(bare.frameless());
        assert!(!framed.frameless());
    }

    #[test]
    fn a_window_of_the_test_is_a_target_for_a_click() {
        assert_eq!(judge_target(Some(Win(5)), &[Win(4), Win(5)]), Ok(()));
    }

    #[test]
    fn a_window_the_test_did_not_make_is_refused_and_named() {
        let said = judge_target(Some(Win(9)), &[Win(5)]).err();
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("Win(9)") && text.contains("not one of")),
            "{said:?}"
        );
    }

    #[test]
    fn no_window_at_the_point_and_an_empty_list_are_both_refused() {
        assert!(judge_target(None, &[Win(5)]).is_err());
        assert!(judge_target(Some(Win(5)), &[]).is_err());
    }

    #[test]
    fn a_dpi_of_zero_is_an_error_and_144_is_one_and_a_half() -> Result<(), String> {
        assert!(scale_from_dpi(0).is_err());
        assert!((scale_from_dpi(144)? - 1.5).abs() < f64::EPSILON);
        Ok(())
    }
}
