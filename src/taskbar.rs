//! Windows 10/11 Taskbar Thumbnail Toolbar controls (Previous / Play-Pause / Next).
//!
//! Uses COM `ITaskbarList3::ThumbBarAddButtons` & `ThumbBarUpdateButtons` with bespoke
//! 4×4 supersampled SDF Fluent icons that automatically adapt to Windows Dark/Light mode.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver, Sender};

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::Shell::{
    THBF_ENABLED, THBN_CLICKED, THB_FLAGS, THB_ICON, THB_TOOLTIP, THUMBBUTTON,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, ChangeWindowMessageFilterEx, CreateIconIndirect, DefWindowProcW, DestroyIcon,
    RegisterWindowMessageW, SetWindowLongPtrW, GWLP_WNDPROC, HICON, ICONINFO, MSGFLT_ALLOW,
    WM_COMMAND, WM_NCDESTROY, WM_SETTINGCHANGE,
};

use crate::i18n::t;

/// Events clicked on the taskbar thumbnail toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarEvent {
    Previous,
    PlayPause,
    Next,
}

const BTN_PREV: u32 = 1001;
const BTN_PLAY_PAUSE: u32 = 1002;
const BTN_NEXT: u32 = 1003;

const CLSID_TASKBAR_LIST: GUID = GUID {
    data1: 0x56FD_F344,
    data2: 0xFD6D,
    data3: 0x11D0,
    data4: [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90],
};

const IID_ITASKBAR_LIST3: GUID = GUID {
    data1: 0xEA1A_FB91,
    data2: 0x9E28,
    data3: 0x4B86,
    data4: [0x90, 0xE9, 0x9E, 0x9F, 0x8A, 0x5E, 0xEF, 0xAF],
};

/// Exact COM vtable layout of `ITaskbarList3` (inherits IUnknown -> ITaskbarList -> ITaskbarList2 -> ITaskbarList3).
#[repr(C)]
struct ITaskbarList3Vtbl {
    // IUnknown
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    // ITaskbarList
    hr_init: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    add_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    delete_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    activate_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    set_active_alt: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    // ITaskbarList2
    mark_fullscreen_window: unsafe extern "system" fn(*mut c_void, HWND, BOOL) -> HRESULT,
    // ITaskbarList3
    set_progress_value: unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> HRESULT,
    set_progress_state: unsafe extern "system" fn(*mut c_void, HWND, i32) -> HRESULT,
    register_tab: unsafe extern "system" fn(*mut c_void, HWND, HWND) -> HRESULT,
    unregister_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    set_tab_order: unsafe extern "system" fn(*mut c_void, HWND, HWND) -> HRESULT,
    set_tab_active: unsafe extern "system" fn(*mut c_void, HWND, HWND, u32) -> HRESULT,
    thumb_bar_add_buttons: unsafe extern "system" fn(*mut c_void, HWND, u32, *const THUMBBUTTON) -> HRESULT,
    thumb_bar_update_buttons: unsafe extern "system" fn(*mut c_void, HWND, u32, *const THUMBBUTTON) -> HRESULT,
    thumb_bar_set_image_list: unsafe extern "system" fn(*mut c_void, HWND, *mut c_void) -> HRESULT,
    set_overlay_icon: unsafe extern "system" fn(*mut c_void, HWND, HICON, *const u16) -> HRESULT,
    set_thumbnail_tooltip: unsafe extern "system" fn(*mut c_void, HWND, *const u16) -> HRESULT,
    set_thumbnail_clip: unsafe extern "system" fn(*mut c_void, HWND, *const RECT) -> HRESULT,
}

#[repr(C)]
struct ITaskbarList3 {
    vtbl: *const ITaskbarList3Vtbl,
}

impl ITaskbarList3 {
    unsafe fn create() -> Option<*mut ITaskbarList3> {
        let _ = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        let mut raw: *mut c_void = null_mut();
        let hr = CoCreateInstance(
            &CLSID_TASKBAR_LIST,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_ITASKBAR_LIST3,
            &mut raw,
        );
        if hr < 0 || raw.is_null() {
            return None;
        }
        let tbl = raw as *mut ITaskbarList3;
        let init_hr = ((*(*tbl).vtbl).hr_init)(raw);
        if init_hr < 0 {
            ((*(*tbl).vtbl).release)(raw);
            return None;
        }
        Some(tbl)
    }

    unsafe fn add_buttons(tbl: *mut ITaskbarList3, hwnd: HWND, buttons: &[THUMBBUTTON]) -> bool {
        ((*(*tbl).vtbl).thumb_bar_add_buttons)(tbl as *mut c_void, hwnd, buttons.len() as u32, buttons.as_ptr()) >= 0
    }

    unsafe fn update_buttons(tbl: *mut ITaskbarList3, hwnd: HWND, buttons: &[THUMBBUTTON]) -> bool {
        ((*(*tbl).vtbl).thumb_bar_update_buttons)(tbl as *mut c_void, hwnd, buttons.len() as u32, buttons.as_ptr()) >= 0
    }

    unsafe fn set_tooltip(tbl: *mut ITaskbarList3, hwnd: HWND, tip: &str) {
        let wide: Vec<u16> = tip.encode_utf16().chain(Some(0)).collect();
        let _ = ((*(*tbl).vtbl).set_thumbnail_tooltip)(tbl as *mut c_void, hwnd, wide.as_ptr());
    }

    unsafe fn release(tbl: *mut ITaskbarList3) {
        if !tbl.is_null() {
            ((*(*tbl).vtbl).release)(tbl as *mut c_void);
        }
    }
}

struct Icons {
    light_theme: bool,
    prev: HICON,
    play: HICON,
    pause: HICON,
    next: HICON,
}

impl Icons {
    fn build(light_theme: bool) -> Self {
        // Dark taskbar -> crisp pure white; Light taskbar -> crisp deep charcoal
        let rgb = if light_theme { (24u8, 24u8, 28u8) } else { (255u8, 255u8, 255u8) };
        Self {
            light_theme,
            prev: make_icon(32, rgb, IconKind::Prev),
            play: make_icon(32, rgb, IconKind::Play),
            pause: make_icon(32, rgb, IconKind::Pause),
            next: make_icon(32, rgb, IconKind::Next),
        }
    }

    fn destroy(&mut self) {
        unsafe {
            for h in [self.prev, self.play, self.pause, self.next] {
                if !h.is_null() {
                    DestroyIcon(h);
                }
            }
        }
        self.prev = null_mut();
        self.play = null_mut();
        self.pause = null_mut();
        self.next = null_mut();
    }
}

impl Drop for Icons {
    fn drop(&mut self) {
        self.destroy();
    }
}

struct HookState {
    original: isize,
    msg_taskbar_created: u32,
    tbl: *mut ITaskbarList3,
    icons: Icons,
    playing: bool,
    added: bool,
    tx: Sender<TaskbarEvent>,
}

impl HookState {
    fn make_buttons(&self) -> [THUMBBUTTON; 3] {
        let (pp_icon, pp_tip) = if self.playing {
            (self.icons.pause, t("Pause"))
        } else {
            (self.icons.play, t("Play"))
        };
        [
            thumb_btn(BTN_PREV, self.icons.prev, t("Previous")),
            thumb_btn(BTN_PLAY_PAUSE, pp_icon, pp_tip),
            thumb_btn(BTN_NEXT, self.icons.next, t("Next")),
        ]
    }

    fn apply_buttons(&mut self, hwnd: HWND, force_add: bool) {
        if self.tbl.is_null() {
            return;
        }
        let buttons = self.make_buttons();
        unsafe {
            if force_add || !self.added {
                if ITaskbarList3::add_buttons(self.tbl, hwnd, &buttons) {
                    self.added = true;
                } else {
                    let _ = ITaskbarList3::update_buttons(self.tbl, hwnd, &buttons);
                    self.added = true;
                }
            } else {
                let _ = ITaskbarList3::update_buttons(self.tbl, hwnd, &buttons);
            }
        }
    }

    fn refresh_theme(&mut self, hwnd: HWND) {
        let light = is_system_light_theme();
        if light != self.icons.light_theme {
            self.icons.destroy();
            self.icons = Icons::build(light);
            self.apply_buttons(hwnd, false);
        }
    }
}

impl Drop for HookState {
    fn drop(&mut self) {
        unsafe {
            ITaskbarList3::release(self.tbl);
        }
        self.tbl = null_mut();
    }
}

thread_local! {
    static HOOKED: RefCell<HashMap<isize, HookState>> = RefCell::new(HashMap::new());
}

/// Windows Taskbar thumbnail media controls attached to the main window.
pub struct Taskbar {
    hwnd: isize,
    rx: Receiver<TaskbarEvent>,
}

impl Taskbar {
    pub fn new(window: &Window) -> Option<Self> {
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return None };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else { return None };
        let hwnd_val = handle.hwnd.get();
        let hwnd = hwnd_val as HWND;

        let tbl = unsafe { ITaskbarList3::create()? };
        let msg_name: Vec<u16> = "TaskbarButtonCreated\0".encode_utf16().collect();
        let msg_taskbar_created = unsafe { RegisterWindowMessageW(msg_name.as_ptr()) };

        unsafe {
            ChangeWindowMessageFilterEx(hwnd, msg_taskbar_created, MSGFLT_ALLOW, null_mut());
            ChangeWindowMessageFilterEx(hwnd, WM_COMMAND, MSGFLT_ALLOW, null_mut());
        }

        let (tx, rx) = mpsc::channel();
        let icons = Icons::build(is_system_light_theme());

        let ours: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = wnd_proc;
        let original = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours as usize as isize) };
        if original == 0 {
            unsafe { ITaskbarList3::release(tbl) };
            return None;
        }

        let mut state = HookState {
            original,
            msg_taskbar_created,
            tbl,
            icons,
            playing: false,
            added: false,
            tx,
        };
        // Register immediately if taskbar button already exists; TaskbarButtonCreated will re-add if needed.
        state.apply_buttons(hwnd, true);

        HOOKED.with(|h| {
            h.borrow_mut().insert(hwnd_val, state);
        });

        Some(Self { hwnd: hwnd_val, rx })
    }

    /// Non-blocking poll for taskbar button clicks.
    pub fn events(&self) -> Vec<TaskbarEvent> {
        self.rx.try_iter().collect()
    }

    /// Updates the center Play / Pause button icon and tooltip.
    pub fn set_playing(&self, playing: bool) {
        HOOKED.with(|h| {
            if let Some(state) = h.borrow_mut().get_mut(&self.hwnd) {
                if state.playing != playing {
                    state.playing = playing;
                    state.apply_buttons(self.hwnd as HWND, false);
                }
            }
        });
    }

    /// Updates the hover tooltip shown on the taskbar thumbnail preview.
    pub fn set_tooltip(&self, tip: &str) {
        HOOKED.with(|h| {
            if let Some(state) = h.borrow().get(&self.hwnd) {
                if !state.tbl.is_null() {
                    unsafe { ITaskbarList3::set_tooltip(state.tbl, self.hwnd as HWND, tip) };
                }
            }
        });
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let key = hwnd as isize;
    enum Action {
        Pass(isize),
        Handled,
        Unhook(isize),
    }

    let action = HOOKED.with(|h| {
        let mut map = h.borrow_mut();
        let state = map.get_mut(&key)?;
        if msg == state.msg_taskbar_created && msg != 0 {
            state.added = false;
            state.apply_buttons(hwnd, true);
            return Some(Action::Pass(state.original));
        }
        match msg {
            WM_COMMAND if ((wparam >> 16) & 0xFFFF) as u32 == THBN_CLICKED => {
                match (wparam & 0xFFFF) as u32 {
                    BTN_PREV => {
                        let _ = state.tx.send(TaskbarEvent::Previous);
                    }
                    BTN_PLAY_PAUSE => {
                        let _ = state.tx.send(TaskbarEvent::PlayPause);
                    }
                    BTN_NEXT => {
                        let _ = state.tx.send(TaskbarEvent::Next);
                    }
                    _ => return Some(Action::Pass(state.original)),
                }
                Some(Action::Handled)
            }
            WM_SETTINGCHANGE => {
                state.refresh_theme(hwnd);
                Some(Action::Pass(state.original))
            }
            WM_NCDESTROY => {
                let original = state.original;
                map.remove(&key);
                Some(Action::Unhook(original))
            }
            _ => Some(Action::Pass(state.original)),
        }
    });

    let call = |original: isize| {
        let f: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = std::mem::transmute(original);
        CallWindowProcW(Some(f), hwnd, msg, wparam, lparam)
    };

    match action {
        None => DefWindowProcW(hwnd, msg, wparam, lparam),
        Some(Action::Handled) => 0,
        Some(Action::Pass(original)) => call(original),
        Some(Action::Unhook(original)) => {
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, original);
            call(original)
        }
    }
}

fn thumb_btn(id: u32, hicon: HICON, tip: &str) -> THUMBBUTTON {
    let mut sz_tip = [0u16; 260];
    for (dst, src) in sz_tip.iter_mut().take(259).zip(tip.encode_utf16()) {
        *dst = src;
    }
    THUMBBUTTON {
        dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
        iId: id,
        iBitmap: 0,
        hIcon: hicon,
        szTip: sz_tip,
        dwFlags: THBF_ENABLED,
    }
}

fn is_system_light_theme() -> bool {
    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let value: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();
    let mut data: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    status == 0 && data != 0
}

// =========================================================================
// Bespoke Windows 11 Fluent SDF Media Icons (4x4 supersampled 32x32 BGRA)
// =========================================================================

#[derive(Clone, Copy)]
enum IconKind {
    Prev,
    Play,
    Pause,
    Next,
}

fn sd_capsule(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let pax = px - x0;
    let pay = py - y0;
    let bax = x1 - x0;
    let bay = y1 - y0;
    let denom = (bax * bax + bay * bay).max(1e-6);
    let h = ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0);
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    (dx * dx + dy * dy).sqrt() - r
}

fn sd_rounded_triangle(
    px: f32,
    py: f32,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    cx: f32,
    cy: f32,
    r: f32,
) -> f32 {
    let (e0x, e0y) = (bx - ax, by - ay);
    let (e1x, e1y) = (cx - bx, cy - by);
    let (e2x, e2y) = (ax - cx, ay - cy);
    let (v0x, v0y) = (px - ax, py - ay);
    let (v1x, v1y) = (px - bx, py - by);
    let (v2x, v2y) = (px - cx, py - cy);

    let dot2 = |x: f32, y: f32| x * x + y * y;
    let t0 = ((v0x * e0x + v0y * e0y) / dot2(e0x, e0y)).clamp(0.0, 1.0);
    let t1 = ((v1x * e1x + v1y * e1y) / dot2(e1x, e1y)).clamp(0.0, 1.0);
    let t2 = ((v2x * e2x + v2y * e2y) / dot2(e2x, e2y)).clamp(0.0, 1.0);

    let (pq0x, pq0y) = (v0x - e0x * t0, v0y - e0y * t0);
    let (pq1x, pq1y) = (v1x - e1x * t1, v1y - e1y * t1);
    let (pq2x, pq2y) = (v2x - e2x * t2, v2y - e2y * t2);

    let s = (e0x * e2y - e0y * e2x).signum();
    let d0 = (dot2(pq0x, pq0y), s * (v0x * e0y - v0y * e0x));
    let d1 = (dot2(pq1x, pq1y), s * (v1x * e1y - v1y * e1x));
    let d2 = (dot2(pq2x, pq2y), s * (v2x * e2y - v2y * e2x));

    let min_d2 = d0.0.min(d1.0).min(d2.0);
    let min_s = d0.1.min(d1.1).min(d2.1);
    -min_d2.sqrt() * min_s.signum() - r
}

fn eval_icon_sdf(x: f32, y: f32, kind: IconKind) -> f32 {
    match kind {
        // Modern Fluent Play: softly rounded equilateral triangle, optically shifted right
        IconKind::Play => sd_rounded_triangle(x, y, -0.32, -0.44, -0.32, 0.44, 0.44, 0.0, 0.095),
        // Modern Fluent Pause: two crisp symmetrical rounded vertical pills
        IconKind::Pause => {
            let left = sd_capsule(x, y, -0.24, -0.36, -0.24, 0.36, 0.11);
            let right = sd_capsule(x, y, 0.24, -0.36, 0.24, 0.36, 0.11);
            left.min(right)
        }
        // Modern Fluent Previous (|◀): clean thin rounded bar + rounded left triangle
        IconKind::Prev => {
            let bar = sd_capsule(x, y, -0.42, -0.35, -0.42, 0.35, 0.068);
            let tri = sd_rounded_triangle(x, y, -0.22, 0.0, 0.40, -0.35, 0.40, 0.35, 0.075);
            bar.min(tri)
        }
        // Modern Fluent Next (▶|): rounded right triangle + clean thin rounded bar
        IconKind::Next => {
            let tri = sd_rounded_triangle(x, y, 0.22, 0.0, -0.40, -0.35, -0.40, 0.35, 0.075);
            let bar = sd_capsule(x, y, 0.42, -0.35, 0.42, 0.35, 0.068);
            tri.min(bar)
        }
    }
}

fn make_icon(size: i32, (r, g, b): (u8, u8, u8), kind: IconKind) -> HICON {
    let mut bgra = vec![0u8; (size * size * 4) as usize];
    let inv_size = 2.0 / size as f32;
    let aa_width = inv_size * 0.45;

    for py in 0..size {
        for px in 0..size {
            // 4x4 supersampling for silky anti-aliased vector quality
            let mut alpha_sum = 0.0f32;
            for sy in 0..4 {
                let fy = (py as f32 + (sy as f32 + 0.5) * 0.25) * inv_size - 1.0;
                for sx in 0..4 {
                    let fx = (px as f32 + (sx as f32 + 0.5) * 0.25) * inv_size - 1.0;
                    let sd = eval_icon_sdf(fx, fy, kind);
                    let cov = ((0.5 - sd / aa_width)).clamp(0.0, 1.0);
                    alpha_sum += cov;
                }
            }
            let a = (alpha_sum / 16.0).clamp(0.0, 1.0);
            let a_u8 = (a * 255.0).round() as u8;
            // Premultiplied BGRA for crisp alpha-blended HICON
            let idx = ((py * size + px) * 4) as usize;
            bgra[idx] = ((b as f32) * a).round() as u8;
            bgra[idx + 1] = ((g as f32) * a).round() as u8;
            bgra[idx + 2] = ((r as f32) * a).round() as u8;
            bgra[idx + 3] = a_u8;
        }
    }

    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size, // top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [std::mem::zeroed()],
        };
        let mut bits: *mut c_void = null_mut();
        let hbm_color = CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if hbm_color.is_null() || bits.is_null() {
            return null_mut();
        }
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());

        let hbm_mask = CreateBitmap(size, size, 1, 1, null());
        let ii = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };
        let hicon = CreateIconIndirect(&ii);
        DeleteObject(hbm_color);
        if !hbm_mask.is_null() {
            DeleteObject(hbm_mask);
        }
        hicon
    }
}
