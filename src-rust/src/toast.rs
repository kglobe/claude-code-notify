//! Toast window: rendering, WndProc, timers, mouse interaction, stacking.
//!
//! Implements the full toast notification window with GDI drawing,
//! fade-out animation, Telegram-style stacking, and click-to-activate.

use std::cell::RefCell;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// --- Constants (SPEC Sections 8.2, 8.3, 10.1, 10.2) ---

// Layout in logical px (96 DPI); scaled per monitor at runtime.
const WINDOW_WIDTH: i32 = 380;
const PAD: i32 = 16;
const RADIUS: f32 = 8.0;
const LOGO_SIZE: i32 = 16;
const BADGE_SIZE: i32 = 22;
const TEXT_LEFT: i32 = PAD + BADGE_SIZE + 12;
const CLOSE_BUTTON_SIZE: i32 = 28;
const CLOSE_BUTTON_MARGIN: i32 = 8;
const STACK_GAP: i32 = 8;
const EDGE_MARGIN: i32 = 12;
/// Transparent band around the card that holds the drop shadow.
const SHADOW: i32 = 12;

fn px(v: i32, scale: f32) -> i32 {
    (v as f32 * scale).round() as i32
}

/// Gap between stacked windows; negative because the shadow bands overlap.
fn stack_gap(scale: f32) -> i32 {
    px(STACK_GAP, scale) - 2 * px(SHADOW, scale)
}

/// Fluent-style palette (0xRRGGBB), picked to keep body text >= 4.5:1 contrast.
struct Theme {
    bg: u32,
    bg_hover: u32,
    border: u32,
    title: u32,
    body: u32,
    muted: u32,
    ok: u32,
    warn: u32,
    on_badge: u32,
    close_hover: u32,
}

const LIGHT: Theme = Theme {
    bg: 0xFBFBFB, bg_hover: 0xF5F5F5, border: 0xDDDDDD, title: 0x1B1B1B, body: 0x3D3D3D,
    muted: 0x6B6B6B, ok: 0x0F7B0F, warn: 0x9D5D00, on_badge: 0xFFFFFF, close_hover: 0xE6E6E6,
};
const DARK: Theme = Theme {
    bg: 0x2B2B2B, bg_hover: 0x323232, border: 0x454545, title: 0xFFFFFF, body: 0xDADADA,
    muted: 0xA8A8A8, ok: 0x6CCB5F, warn: 0xFCE100, on_badge: 0x1B1B1B, close_hover: 0x3D3D3D,
};

const TIMER_FADE: usize = 1;
const TIMER_START_FADE: usize = 2;
const TIMER_REPOSITION: usize = 3;
const TIMER_CHECK_BOTTOM: usize = 4;

const DISPLAY_MS: u32 = 6000;
const FADE_MS: u32 = 600;
const INITIAL_ALPHA: u8 = 255;

const TOAST_CLASS_NAME: &str = "ClaudeCodeToast";

const WM_TOAST_CHECK_POSITION: u32 = WM_USER + 101;
const WM_TOAST_PAUSE_TIMER: u32 = WM_USER + 102;
const WM_MOUSELEAVE: u32 = 0x02A3;

// --- Global state for the toast window (per-process, one toast per process) ---

struct ToastState {
    hwnd: HWND,
    title: String,
    status: String,
    message: String,
    cwd: String,
    time: String,
    input_mode: bool,
    icon: HICON,
    default_icon_path: String,
    // Rendering (per-pixel alpha surface for UpdateLayeredWindow)
    scale: f32,
    width: i32,
    height: i32,
    mem_dc: HDC,
    bits: *mut u32,
    hover_close: bool,
    // Activation targets
    target_hwnd: HWND,
    wt_hwnd: HWND,
    wt_runtime_id: String,
    // Fade state
    alpha: u8,
    fade_step: u8,
    is_fading: bool,
    // Mouse state
    mouse_inside: bool,
    // Stacking state
    target_y: i32,
    is_bottom_toast: bool,
    taskbar_edge: u32,
    // Clicked flag
    clicked: bool,
}

thread_local! {
    static TOAST: RefCell<Option<ToastState>> = const { RefCell::new(None) };
}

/// Execute a closure with an immutable reference to the toast state.
fn with_toast<R>(f: impl FnOnce(&ToastState) -> R) -> R {
    TOAST.with(|cell| {
        let borrow = cell.borrow();
        f(borrow.as_ref().unwrap())
    })
}

/// Execute a closure with a mutable reference to the toast state.
fn with_toast_mut<R>(f: impl FnOnce(&mut ToastState) -> R) -> R {
    TOAST.with(|cell| {
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().unwrap())
    })
}

fn make_font(height: i32, weight: i32, family: &str) -> HFONT {
    let face = crate::util::encode_wide(family);
    unsafe {
        CreateFontW(
            -height, 0, 0, 0, // negative = character height, matches CSS px
            weight,
            0, 0, 0,
            FONT_CHARSET(1), // DEFAULT_CHARSET so CJK font linking kicks in
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            ANTIALIASED_QUALITY, // grayscale: no colour fringes on the tinted card
            0,
            PCWSTR(face.as_ptr()),
        )
    }
}

/// Close button rect in device px, relative to the card (not the window).
fn close_rect(width: i32, scale: f32) -> RECT {
    let s = |v: i32| (v as f32 * scale).round() as i32;
    let left = width - s(CLOSE_BUTTON_MARGIN + CLOSE_BUTTON_SIZE);
    let top = s(CLOSE_BUTTON_MARGIN);
    RECT { left, top, right: left + s(CLOSE_BUTTON_SIZE), bottom: top + s(CLOSE_BUTTON_SIZE) }
}

fn is_point_in_close_button(x: i32, y: i32) -> bool {
    let (r, o) = with_toast(|t| {
        let o = px(SHADOW, t.scale);
        (close_rect(t.width - 2 * o, t.scale), o)
    });
    let (x, y) = (x - o, y - o);
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// Re-apply the window's per-pixel surface with a new overall alpha.
fn set_alpha(hwnd: HWND, alpha: u8) {
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    unsafe {
        let _ = UpdateLayeredWindow(hwnd, None, None, None, None, None, COLORREF(0), Some(&blend), ULW_ALPHA);
    }
}

fn system_uses_light_theme() -> bool {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    unsafe {
        let _ = windows::Win32::System::Registry::RegGetValueW(
            windows::Win32::System::Registry::HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            windows::Win32::System::Registry::RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        );
    }
    value != 0
}

// --- Stacking helpers ---

struct ToastInfo {
    hwnd: HWND,
    rect: RECT,
}

fn enum_other_toasts() -> Vec<ToastInfo> {
    let mut toasts: Vec<ToastInfo> = Vec::new();

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let toasts = &mut *(lparam.0 as *mut Vec<ToastInfo>);
        let my_hwnd = TOAST.with(|cell| {
            cell.borrow().as_ref().map(|t| t.hwnd).unwrap_or_default()
        });
        if hwnd == my_hwnd {
            return TRUE;
        }
        if crate::util::get_class_name(hwnd) == TOAST_CLASS_NAME && IsWindowVisible(hwnd).as_bool() {
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            toasts.push(ToastInfo { hwnd, rect });
        }
        TRUE
    }

    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut toasts as *mut Vec<ToastInfo> as isize),
        );
    }

    toasts
}

fn calculate_position(work_area: &RECT, taskbar_edge: u32, width: i32, height: i32, margin: i32, gap: i32) -> (i32, i32) {
    let other_toasts = enum_other_toasts();

    // X position (margin keeps the card off the screen edge, like system toasts)
    let x = if taskbar_edge == ABE_LEFT {
        work_area.left + margin
    } else {
        work_area.right - width - margin
    };

    // Y position
    let y = if other_toasts.is_empty() {
        // First toast
        if taskbar_edge == ABE_TOP {
            work_area.top + margin
        } else {
            work_area.bottom - height - margin
        }
    } else {
        if taskbar_edge == ABE_TOP {
            // Stack below: find lowest bottom
            let lowest_bottom = other_toasts.iter().map(|t| t.rect.bottom).max().unwrap_or(work_area.top);
            lowest_bottom + gap
        } else {
            // Stack above: find highest top
            let highest_top = other_toasts.iter().map(|t| t.rect.top).min().unwrap_or(work_area.bottom);
            highest_top - height - gap
        }
    };

    (x, y)
}

fn is_bottom_toast_check(hwnd: HWND, _taskbar_edge: u32) -> bool {
    let other_toasts = enum_other_toasts();
    if other_toasts.is_empty() {
        return true;
    }

    // Bottom toast = the one with the lowest HWND value (created earliest, closest to taskbar)
    for t in &other_toasts {
        if (t.hwnd.0 as usize) < (hwnd.0 as usize) {
            return false;
        }
    }
    true
}

/// The bottom toast is the one nearest the taskbar.
fn is_bottom(_my_top: i32, other_tops: &[i32], _taskbar_top: bool) -> bool {
    other_tops.is_empty()
}

fn notify_other_toasts_closing(my_hwnd: HWND) {
    let mut my_rect = RECT::default();
    unsafe { let _ = GetWindowRect(my_hwnd, &mut my_rect); }

    let others = enum_other_toasts();
    for t in &others {
        unsafe {
            let _ = SendMessageW(
                t.hwnd,
                WM_TOAST_CHECK_POSITION,
                Some(WPARAM(my_rect.top as usize)),
                Some(LPARAM((my_rect.bottom - my_rect.top) as isize)),
            );
        }
    }
}

fn notify_all_toasts_pause_timer(pause: bool) {
    let my_hwnd = with_toast(|t| t.hwnd);
    // Send to self
    unsafe {
        let _ = SendMessageW(
            my_hwnd,
            WM_TOAST_PAUSE_TIMER,
            Some(WPARAM(if pause { 1 } else { 0 })),
            Some(LPARAM(0)),
        );
    }
    // Send to others
    let others = enum_other_toasts();
    for t in &others {
        unsafe {
            let _ = SendMessageW(
                t.hwnd,
                WM_TOAST_PAUSE_TIMER,
                Some(WPARAM(if pause { 1 } else { 0 })),
                Some(LPARAM(0)),
            );
        }
    }
}

fn animate_to_position(hwnd: HWND) {
    let mut rect = RECT::default();
    unsafe { let _ = GetWindowRect(hwnd, &mut rect); }

    let (target_y, done) = with_toast_mut(|state| {
        let current_y = rect.top;
        let diff = state.target_y - current_y;

        if diff == 0 {
            return (0, true);
        }

        let mut step = diff * 2 / 5;
        if step == 0 {
            step = if diff > 0 { 2 } else { -2 };
        }

        let mut new_y = current_y + step;
        if (state.target_y - new_y).abs() < 4 {
            new_y = state.target_y;
        }

        (new_y, new_y == state.target_y)
    });

    if done && target_y == 0 {
        // diff was 0, just kill timer
        unsafe { let _ = KillTimer(Some(hwnd), TIMER_REPOSITION); }
        return;
    }

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left, target_y, 0, 0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    if done {
        unsafe { let _ = KillTimer(Some(hwnd), TIMER_REPOSITION); }
    }
}

// --- WndProc ---

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER => {
            match wparam.0 {
                TIMER_START_FADE => {
                    let _ = KillTimer(Some(hwnd), TIMER_START_FADE);
                    with_toast_mut(|state| state.is_fading = true);
                    SetTimer(Some(hwnd), TIMER_FADE, 16, None);
                }
                TIMER_FADE => {
                    let should_destroy = with_toast_mut(|state| {
                        if state.alpha > state.fade_step {
                            state.alpha -= state.fade_step;
                            set_alpha(hwnd, state.alpha);
                            false
                        } else {
                            state.is_fading = false;
                            true
                        }
                    });
                    if should_destroy {
                        let _ = KillTimer(Some(hwnd), TIMER_FADE);
                        notify_other_toasts_closing(hwnd);
                        let _ = DestroyWindow(hwnd);
                    }
                }
                TIMER_REPOSITION => {
                    animate_to_position(hwnd);
                }
                TIMER_CHECK_BOTTOM => {
                    let taskbar_edge = with_toast(|s| s.taskbar_edge);
                    if is_bottom_toast_check(hwnd, taskbar_edge) {
                        with_toast_mut(|state| {
                            state.is_bottom_toast = true;
                        });
                        let _ = KillTimer(Some(hwnd), TIMER_CHECK_BOTTOM);
                        SetTimer(Some(hwnd), TIMER_START_FADE, DISPLAY_MS, None);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

            if is_point_in_close_button(x, y) {
                // Close button click
                let _ = KillTimer(Some(hwnd), TIMER_START_FADE);
                let _ = KillTimer(Some(hwnd), TIMER_FADE);
                notify_other_toasts_closing(hwnd);
                let _ = DestroyWindow(hwnd);
            } else {
                // Body click: activate window
                let _ = KillTimer(Some(hwnd), TIMER_START_FADE);
                let _ = KillTimer(Some(hwnd), TIMER_FADE);
                notify_other_toasts_closing(hwnd);
                let _ = ShowWindow(hwnd, SW_HIDE);

                let (target, wt, rid) = with_toast_mut(|state| {
                    state.clicked = true;
                    (state.target_hwnd, state.wt_hwnd, state.wt_runtime_id.clone())
                });
                crate::activate::activate_window(target, wt, &rid);

                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }

        WM_RBUTTONUP => {
            // Right click: close without activation
            let _ = KillTimer(Some(hwnd), TIMER_START_FADE);
            let _ = KillTimer(Some(hwnd), TIMER_FADE);
            notify_other_toasts_closing(hwnd);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let was_inside = with_toast(|s| s.mouse_inside);
            if !was_inside {
                with_toast_mut(|state| state.mouse_inside = true);
                // Track mouse leave
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);

                // Pause all toasts
                notify_all_toasts_pause_timer(true);
            }
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let over_close = is_point_in_close_button(x, y);
            let changed = with_toast_mut(|state| {
                let changed = !was_inside || state.hover_close != over_close;
                state.hover_close = over_close;
                changed
            });
            if changed {
                render(hwnd);
            }
            LRESULT(0)
        }

        WM_MOUSELEAVE => {
            with_toast_mut(|state| {
                state.mouse_inside = false;
                state.hover_close = false;
            });
            render(hwnd);
            // Resume all toasts
            notify_all_toasts_pause_timer(false);
            LRESULT(0)
        }

        x if x == WM_TOAST_CHECK_POSITION => {
            let closed_toast_y = wparam.0 as i32;
            let mut my_rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut my_rect);

            let taskbar_edge = with_toast(|s| s.taskbar_edge);

            // Update position target (borrow released before is_bottom_toast_check)
            let closed_height = lparam.0 as i32;
            with_toast_mut(|state| {
                let shift = closed_height + stack_gap(state.scale);
                if state.taskbar_edge == ABE_TOP {
                    // Top taskbar: if we're below the closed toast, move up
                    if my_rect.top > closed_toast_y {
                        state.target_y = my_rect.top - shift;
                        SetTimer(Some(hwnd), TIMER_REPOSITION, 16, None);
                    }
                } else {
                    // Bottom taskbar: if we're above the closed toast, move down
                    if my_rect.top < closed_toast_y {
                        state.target_y = my_rect.top + shift;
                        SetTimer(Some(hwnd), TIMER_REPOSITION, 16, None);
                    }
                }
            });

            // Check bottom toast status OUTSIDE the borrow to avoid double-borrow
            // (is_bottom_toast_check -> enum_other_toasts -> callback borrows TOAST)
            let is_already_bottom = with_toast(|s| s.is_bottom_toast);
            if !is_already_bottom && is_bottom_toast_check(hwnd, taskbar_edge) {
                with_toast_mut(|state| {
                    state.is_bottom_toast = true;
                    let _ = KillTimer(Some(hwnd), TIMER_CHECK_BOTTOM);
                    if !state.mouse_inside {
                        SetTimer(Some(hwnd), TIMER_START_FADE, DISPLAY_MS, None);
                    }
                });
            }

            LRESULT(0)
        }

        x if x == WM_TOAST_PAUSE_TIMER => {
            let pause = wparam.0 == 1;

            with_toast_mut(|state| {
                if pause {
                    if state.is_fading {
                        let _ = KillTimer(Some(hwnd), TIMER_FADE);
                        state.is_fading = false;
                        state.alpha = INITIAL_ALPHA;
                        set_alpha(hwnd, INITIAL_ALPHA);
                    }
                    let _ = KillTimer(Some(hwnd), TIMER_START_FADE);
                } else {
                    // Resume: only start fade timer if bottom toast and mouse not inside
                    if state.is_bottom_toast && !state.mouse_inside {
                        SetTimer(Some(hwnd), TIMER_START_FADE, DISPLAY_MS, None);
                    }
                }
            });
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// --- Render ---

fn cref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF))
}

fn lerp_rgb(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let x = ((a >> s) & 0xFF) as f32;
        let y = ((b >> s) & 0xFF) as f32;
        ((x + (y - x) * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// Signed distance from a pixel center to a rounded box (negative = inside).
fn sdf_rrect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

fn coverage(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Vertical layout in device px, derived from logical constants and message length.
struct Layout {
    header_y: i32,
    title_y: i32,
    status_y: i32,
    message_y: i32,
    message_h: i32,
    path_y: i32,
    height: i32,
}

unsafe fn measure_layout(dc: HDC, scale: f32, width: i32, message: &str) -> Layout {
    let s = |v: i32| (v as f32 * scale).round() as i32;
    let header_y = s(12);
    let title_y = s(38);
    let status_y = s(64);
    let message_y = s(86);

    let mut message_h = 0;
    if !message.is_empty() {
        let font = make_font(s(14), 400, "Segoe UI");
        let old = SelectObject(dc, HGDIOBJ(font.0));
        let mut r = RECT { left: 0, top: 0, right: width - s(TEXT_LEFT) - s(PAD), bottom: 0 };
        let mut buf = crate::util::encode_wide(message);
        let len = buf.len() - 1;
        DrawTextW(dc, &mut buf[..len], &mut r, DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX);
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(font.0));
        message_h = r.bottom.min(s(40)).max(s(20)); // 1-2 lines
    }

    let path_y = if message_h > 0 { message_y + message_h + s(6) } else { message_y };
    let height = path_y + s(16) + s(14);
    Layout { header_y, title_y, status_y, message_y, message_h, path_y, height }
}

unsafe fn draw_text(dc: HDC, text: &str, rect: RECT, size: i32, weight: i32, family: &str, color: u32, flags: DRAW_TEXT_FORMAT) {
    if text.is_empty() {
        return;
    }
    let font = make_font(size, weight, family);
    let old = SelectObject(dc, HGDIOBJ(font.0));
    SetTextColor(dc, cref(color));
    let mut r = rect;
    let mut buf = crate::util::encode_wide(text);
    let len = buf.len() - 1;
    DrawTextW(dc, &mut buf[..len], &mut r, flags | DT_NOPREFIX);
    SelectObject(dc, old);
    let _ = DeleteObject(HGDIOBJ(font.0));
}

/// Paint the whole card into the DIB, then push it to the layered window.
unsafe fn render(hwnd: HWND) {
    let (title, status, message, cwd, time, input_mode, icon, default_icon_path,
         scale, w, h, dc, bits, hover, hover_close, alpha) = with_toast(|t| (
        t.title.clone(), t.status.clone(), t.message.clone(), t.cwd.clone(), t.time.clone(),
        t.input_mode, t.icon, t.default_icon_path.clone(),
        t.scale, t.width, t.height, t.mem_dc, t.bits, t.mouse_inside, t.hover_close, t.alpha,
    ));
    let light = system_uses_light_theme();
    let theme = if light { &LIGHT } else { &DARK };
    let s = |v: i32| (v as f32 * scale).round() as i32;
    let sf = |v: f32| v * scale;
    // Card sits inside the shadow band; GDI draws in card coords via the viewport origin.
    let o = s(SHADOW);
    let (cw, ch) = (w - 2 * o, h - 2 * o);
    let layout = measure_layout(dc, scale, cw, &message);
    let accent = if input_mode { theme.warn } else { theme.ok };

    // Pass 1: shapes (background, hairline border, status badge, close hover), anti-aliased via SDF.
    let px = std::slice::from_raw_parts_mut(bits, (w * h) as usize);
    let (cx, cy, hw, hh) = (w as f32 / 2.0, h as f32 / 2.0, cw as f32 / 2.0, ch as f32 / 2.0);
    let r = sf(RADIUS);
    let bw = sf(1.0).max(1.0);
    let of = o as f32;
    let badge_r = sf(BADGE_SIZE as f32) / 2.0;
    let badge_cx = (s(PAD) as f32) + badge_r;
    let badge_cy = (layout.title_y + s(13)) as f32;
    let close = close_rect(cw, scale);
    let bg = if hover { theme.bg_hover } else { theme.bg };
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = sdf_rrect(fx, fy, cx, cy, hw, hh, r);
            let mut c = lerp_rgb(bg, theme.border, coverage(-(d + bw)));
            let db = ((fx - of - badge_cx).powi(2) + (fy - of - badge_cy).powi(2)).sqrt() - badge_r;
            c = lerp_rgb(c, accent, coverage(db));
            if hover_close {
                let dc_ = sdf_rrect(fx - of, fy - of,
                    (close.left + close.right) as f32 / 2.0, (close.top + close.bottom) as f32 / 2.0,
                    (close.right - close.left) as f32 / 2.0, (close.bottom - close.top) as f32 / 2.0, sf(4.0));
                c = lerp_rgb(c, theme.close_hover, coverage(dc_));
            }
            px[(y * w + x) as usize] = c;
        }
    }

    // Pass 2: GDI content on top of the opaque card.
    SetBkMode(dc, TRANSPARENT);
    let _ = SetViewportOrgEx(dc, o, o, None);
    let logo_x = s(PAD);
    let logo_y = layout.header_y + s(2);
    let logo = s(LOGO_SIZE);
    let mut drawn = false;
    if !default_icon_path.is_empty() {
        let path_wide = crate::util::encode_wide(&default_icon_path);
        if let Ok(handle) = LoadImageW(None, PCWSTR(path_wide.as_ptr()), IMAGE_ICON, logo, logo, LR_LOADFROMFILE) {
            let h_icon = HICON(handle.0);
            if !h_icon.is_invalid() {
                drawn = DrawIconEx(dc, logo_x, logo_y, h_icon, logo, logo, 0, None, DI_NORMAL).is_ok();
                let _ = DestroyIcon(h_icon);
            }
        }
    }
    if !drawn && !icon.is_invalid() {
        let _ = DrawIconEx(dc, logo_x, logo_y, icon, logo, logo, 0, None, DI_NORMAL);
    }

    let header = RECT { left: logo_x + logo + s(8), top: layout.header_y, right: close.left - s(4), bottom: layout.header_y + s(20) };
    draw_text(dc, "Claude Code", header, s(12), 600, "Segoe UI", theme.muted, DT_SINGLELINE | DT_VCENTER);
    draw_text(dc, &time, header, s(12), 400, "Segoe UI", theme.muted, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);

    // Close glyph (Fluent "ChromeClose")
    draw_text(dc, "\u{E8BB}", close, s(10), 400, "Segoe Fluent Icons",
        if hover_close { theme.title } else { theme.muted }, DT_SINGLELINE | DT_VCENTER | DT_CENTER);

    // Badge glyph: check mark for done, exclamation for needs-input
    let badge = RECT {
        left: (badge_cx - badge_r) as i32, top: (badge_cy - badge_r) as i32,
        right: (badge_cx + badge_r).ceil() as i32, bottom: (badge_cy + badge_r).ceil() as i32,
    };
    if input_mode {
        draw_text(dc, "!", badge, s(15), 800, "Segoe UI", theme.on_badge, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
    } else {
        draw_text(dc, "\u{E73E}", badge, s(12), 700, "Segoe Fluent Icons", theme.on_badge, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
    }

    let text_left = s(TEXT_LEFT);
    let text_right = cw - s(PAD);
    draw_text(dc, &title, RECT { left: text_left, top: layout.title_y, right: text_right, bottom: layout.title_y + s(26) },
        s(17), 600, "Segoe UI", theme.title, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    draw_text(dc, &status, RECT { left: text_left, top: layout.status_y, right: text_right, bottom: layout.status_y + s(20) },
        s(13), 600, "Segoe UI", accent, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    if layout.message_h > 0 {
        draw_text(dc, &message,
            RECT { left: text_left, top: layout.message_y, right: text_right, bottom: layout.message_y + layout.message_h },
            s(14), 400, "Segoe UI", theme.body, DT_WORDBREAK | DT_EDITCONTROL | DT_END_ELLIPSIS);
    }
    draw_text(dc, &cwd, RECT { left: text_left, top: layout.path_y, right: text_right, bottom: layout.path_y + s(16) },
        s(12), 400, "Segoe UI", theme.muted, DT_SINGLELINE | DT_VCENTER | DT_PATH_ELLIPSIS);

    let _ = SetViewportOrgEx(dc, 0, 0, None);

    // Pass 3: card alpha mask over a soft black drop shadow, premultiplied (GDI zeroes alpha, so set it last).
    let _ = GdiFlush();
    let (peak, blur, drop) = (if light { 0.16 } else { 0.45 }, sf(9.0), sf(3.0));
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let a = coverage(sdf_rrect(fx, fy, cx, cy, hw, hh, r));
            let t = (sdf_rrect(fx, fy - drop, cx, cy, hw, hh, r) / blur).clamp(0.0, 1.0);
            let sa = peak * (1.0 - t) * (1.0 - t);
            let i = (y * w + x) as usize;
            let c = px[i];
            let pm = |sh: u32| ((((c >> sh) & 0xFF) as f32 * a).round() as u32) << sh;
            let alpha = a + sa * (1.0 - a);
            px[i] = (((alpha * 255.0).round() as u32) << 24) | pm(16) | pm(8) | pm(0);
        }
    }

    let mut wr = RECT::default();
    let _ = GetWindowRect(hwnd, &mut wr);
    let dst = POINT { x: wr.left, y: wr.top };
    let size = SIZE { cx: w, cy: h };
    let src = POINT::default();
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(hwnd, None, Some(&dst), Some(&size), Some(dc), Some(&src),
        COLORREF(0), Some(&blend), ULW_ALPHA);
}

// --- Public API ---

pub struct ToastParams {
    pub title: String,
    pub status: String,
    pub message: String,
    pub cwd: String,
    pub input_mode: bool,
    pub icon: HICON,
    pub default_icon_path: String,
    pub target_hwnd: HWND,
    pub wt_hwnd: HWND,
    pub wt_runtime_id: String,
}

/// Show the toast notification window. Blocks until the window is closed.
pub fn show_toast(params: ToastParams) {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }

    // Calculate fade step (SPEC 10.3)
    let fade_ticks = (FADE_MS / 16).max(1);
    let fade_step = ((INITIAL_ALPHA as u32 / fade_ticks) + 1).min(255) as u8;

    // Detect taskbar position
    let taskbar_edge = detect_taskbar_edge();

    // Get work area from cursor's monitor
    let (work_area, monitor) = get_cursor_monitor_work_area();

    let scale = unsafe {
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = windows::Win32::UI::HiDpi::GetDpiForMonitor(
            monitor, windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        dx as f32 / 96.0
    };
    // Window = card + shadow band on every side.
    let shadow = px(SHADOW, scale);
    let card_w = px(WINDOW_WIDTH, scale);
    let width = card_w + 2 * shadow;
    let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let time = format!("{:02}:{:02}", now.wHour, now.wMinute);

    // Offscreen 32bpp top-down DIB sized to the measured layout.
    let (mem_dc, bits, height) = unsafe {
        let dc = CreateCompatibleDC(None);
        let height = measure_layout(dc, scale, card_w, &params.message).height + 2 * shadow;
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        SelectObject(dc, HGDIOBJ(bmp.0));
        (dc, bits as *mut u32, height)
    };
    if bits.is_null() {
        crate::debug_log!("CreateDIBSection failed");
        return;
    }

    TOAST.with(|cell| {
        *cell.borrow_mut() = Some(ToastState {
            hwnd: HWND::default(),
            title: params.title,
            status: params.status,
            message: params.message,
            cwd: params.cwd,
            time,
            input_mode: params.input_mode,
            icon: params.icon,
            default_icon_path: params.default_icon_path,
            scale,
            width,
            height,
            mem_dc,
            bits,
            hover_close: false,
            target_hwnd: params.target_hwnd,
            wt_hwnd: params.wt_hwnd,
            wt_runtime_id: params.wt_runtime_id,
            alpha: INITIAL_ALPHA,
            fade_step,
            is_fading: false,
            mouse_inside: false,
            target_y: 0,
            is_bottom_toast: false,
            taskbar_edge,
            clicked: false,
        });
    });

    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_wide = crate::util::encode_wide(TOAST_CLASS_NAME);

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_wide.as_ptr()),
            hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
            ..Default::default()
        };

        // OK if already registered by another toast instance
        let _ = RegisterClassExW(&wc);

        let margin = px(EDGE_MARGIN, scale) - shadow;
        let (x, y) = calculate_position(&work_area, taskbar_edge, width, height, margin, stack_gap(scale));

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
            PCWSTR(class_wide.as_ptr()),
            w!("Toast"),
            WS_POPUP,
            x, y, width, height,
            None, None, Some(instance.into()), None,
        ).unwrap_or_default();

        if hwnd.is_invalid() || hwnd == HWND::default() {
            crate::debug_log!("CreateWindowExW failed");
            return;
        }

        with_toast_mut(|state| state.hwnd = hwnd);

        render(hwnd);

        // Determine if bottom toast and start appropriate timer
        if is_bottom_toast_check(hwnd, taskbar_edge) {
            with_toast_mut(|state| state.is_bottom_toast = true);
            SetTimer(Some(hwnd), TIMER_START_FADE, DISPLAY_MS, None);
        } else {
            with_toast_mut(|state| state.is_bottom_toast = false);
            SetTimer(Some(hwnd), TIMER_CHECK_BOTTOM, 200, None);
        }

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = UpdateWindow(hwnd);

        // Message loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn detect_taskbar_edge() -> u32 {
    let mut abd = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };

    let result = unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut abd) };
    if result != 0 {
        abd.uEdge
    } else {
        ABE_BOTTOM
    }
}

fn get_cursor_monitor_work_area() -> (RECT, HMONITOR) {
    unsafe {
        let mut cursor_pos = POINT::default();
        let _ = GetCursorPos(&mut cursor_pos);

        let monitor = MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut mi);

        (mi.rcWork, monitor)
    }
}

#[cfg(test)]
mod tests {
    use super::is_bottom;

    #[test]
    fn bottom_toast_is_the_one_nearest_the_taskbar() {
        // Taskbar at bottom: larger top = closer to taskbar.
        assert!(is_bottom(900, &[700, 500], false));
        assert!(!is_bottom(700, &[900, 500], false));
        // Taskbar at top: smaller top = closer to taskbar.
        assert!(is_bottom(10, &[200, 400], true));
        assert!(!is_bottom(200, &[10, 400], true));
        assert!(is_bottom(500, &[], false));
    }
}
