//! Native pointer: WebView2 embedded in another process's window receives no DOM events (spike S1),
//! so read cursor and buttons every 30 ms and send `pets://pointer` in stage CSS pixels.
//! In "Move" mode and the floating window, the same thread detects dragging (`Dragger`).
use serde::Serialize;
use std::sync::atomic::{AtomicIsize, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// `x`, `y`: CSS pixels relative to stage window; `sx`, `sy`: physical screen pixels (for dragging).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample { pub inside: bool, pub x: f64, pub y: f64, pub sx: i32, pub sy: i32, pub left: bool, pub right: bool }

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointerEvent { Move { x: f64, y: f64 }, Leave, Click { x: f64, y: f64 }, Context { x: f64, y: f64 } }

pub fn diff(prev: &Sample, cur: &Sample) -> Vec<PointerEvent> {
    let mut out = Vec::new();
    if cur.inside {
        if !prev.inside || prev.x != cur.x || prev.y != cur.y { out.push(PointerEvent::Move { x: cur.x, y: cur.y }); }
        if cur.right && !prev.right && prev.inside { out.push(PointerEvent::Context { x: cur.x, y: cur.y }); }
    } else if prev.inside {
        out.push(PointerEvent::Leave);
    }
    out
}

/// Drag from a press in the window: `dx`, `dy` in screen pixels from the press point.
/// `Click`: release without exceeding movement threshold (floating window clicks only on release).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag { Start, Move { dx: i32, dy: i32 }, End, Click { x: f64, y: f64 } }

pub struct Dragger { threshold: i32, down: Option<(i32, i32)>, active: bool, last: (i32, i32) }

impl Dragger {
    pub fn new(threshold: i32) -> Dragger { Dragger { threshold, down: None, active: false, last: (0, 0) } }

    pub fn step(&mut self, prev: &Sample, cur: &Sample) -> Vec<Drag> {
        let mut out = Vec::new();
        if cur.left && !prev.left {
            if cur.inside { self.down = Some((cur.sx, cur.sy)); self.last = (0, 0); }
            if cur.inside && self.threshold == 0 { self.active = true; out.push(Drag::Start); }
            return out;
        }
        let Some((x0, y0)) = self.down else { return out };
        if cur.left {
            let (dx, dy) = (cur.sx - x0, cur.sy - y0);
            if !self.active && (dx.abs() > self.threshold || dy.abs() > self.threshold) { self.active = true; out.push(Drag::Start); }
            if self.active && (dx, dy) != self.last { self.last = (dx, dy); out.push(Drag::Move { dx, dy }); }
        } else {
            out.push(if self.active { Drag::End } else { Drag::Click { x: cur.x, y: cur.y } });
            self.down = None;
            self.active = false;
        }
        out
    }
}

/// Left click in the taskbar: on release, pressed and released inside, like Windows flyouts. Opening the panel
/// on the press let the release that followed hand focus back to the taskbar, which closed the panel at once.
pub fn release_click(d: &mut Dragger, prev: &Sample, cur: &Sample) -> Option<PointerEvent> {
    d.step(prev, cur).into_iter().find_map(|e| match e {
        Drag::Click { x, y } if cur.inside => Some(PointerEvent::Click { x, y }),
        _ => None,
    })
}

/// Floating window currently passes clicks through (cursor over empty space): presses are not ours.
pub static PASSTHROUGH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Cursor "in the stage": window visible and actually under the cursor (not covered by a menu or another window).
/// When the floating window passes clicks through, `WindowFromPoint` points to the window beneath it, so use
/// the rectangle; presses are ignored anyway in that case.
pub fn inside_at(in_rect: bool, visible: bool, hit_ours: bool, by_rect: bool, passthrough: bool) -> bool {
    in_rect && visible && (hit_ours || (by_rect && passthrough))
}

/// Enter/Esc edges in "Move" mode, counted from mode entry: a key already pressed at that point
/// (Enter on the "Move" button) does not count.
#[derive(Default)]
pub struct Keys { prev: Option<(bool, bool)> }

impl Keys {
    pub fn step(&mut self, active: bool, now: (bool, bool)) -> (bool, bool) {
        if !active { self.prev = None; return (false, false); }
        let p = self.prev.unwrap_or(now);
        self.prev = Some(now);
        (now.0 && !p.0, now.1 && !p.1)
    }
}

/// Pointer mode set by the stage loop.
pub const NORMAL: u8 = 0;
/// "Move" in taskbar: drag without a threshold, Enter/click outside confirms, Esc cancels, no UI clicks.
pub const MOVING: u8 = 1;
/// Floating window: drag after 4 px, short click on release.
pub const FLOATING: u8 = 2;

pub fn spawn(app: AppHandle, stage: Arc<AtomicIsize>, mode: Arc<AtomicU8>, tx: Sender<super::Cmd>) {
    std::thread::spawn(move || {
        let mut prev = Sample::default();
        let (mut moving, mut floating, mut clicking) = (Dragger::new(0), Dragger::new(4), Dragger::new(4));
        let mut keys = Keys::default();
        loop {
            std::thread::sleep(Duration::from_millis(30));
            let m = mode.load(Ordering::Relaxed);
            let cur = sample(stage.load(Ordering::Relaxed), m == FLOATING).unwrap_or_default();
            match m {
                MOVING => {
                    for d in moving.step(&prev, &cur) { let _ = tx.send(super::Cmd::Drag(d)); }
                    if cur.left && !prev.left && !cur.inside { let _ = tx.send(super::Cmd::MoveDone(true)); }
                    let (enter, esc) = keys.step(true, (key_down(0x0D), key_down(0x1B)));
                    if enter { let _ = tx.send(super::Cmd::MoveDone(true)); }
                    if esc { let _ = tx.send(super::Cmd::MoveDone(false)); }
                }
                FLOATING => {
                    keys.step(false, (false, false));
                    // clicking empty space goes to the window beneath, not the stage
                    let cur = if PASSTHROUGH.load(Ordering::Relaxed) { Sample { left: false, right: false, ..cur } } else { cur };
                    for e in diff(&prev, &cur) { let _ = app.emit("pets://pointer", e); }
                    for d in floating.step(&prev, &cur) {
                        match d {
                            Drag::Click { x, y } => { let _ = app.emit("pets://pointer", PointerEvent::Click { x, y }); }
                            d => { let _ = tx.send(super::Cmd::Drag(d)); }
                        }
                    }
                }
                _ => {
                    keys.step(false, (false, false));
                    for e in diff(&prev, &cur) { let _ = app.emit("pets://pointer", e); }
                    if let Some(c) = release_click(&mut clicking, &prev, &cur) { let _ = app.emit("pets://pointer", c); }
                }
            }
            // a press still held when the mode changed must not click once the stage is back in the taskbar
            if m != NORMAL { clicking = Dragger::new(4); }
            prev = cur;
        }
    });
}

#[cfg(windows)]
fn key_down(vk: i32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    (unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000) != 0
}

#[cfg(not(windows))]
fn key_down(vk: i32) -> bool { super::taskbar::raw_key_down(vk) }

/// `by_rect`: the floating window may pass pointer events through (`WindowFromPoint` then points beneath it),
/// so determine "inside" from the rectangle alone.
#[cfg(windows)]
fn sample(raw: isize, by_rect: bool) -> Option<Sample> {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect, IsChild, IsWindowVisible, WindowFromPoint};
    if raw == 0 { return None; }
    let h = super::taskbar::hwnd(raw);
    unsafe {
        let mut r = RECT::default();
        GetWindowRect(h, &mut r).ok()?;
        let mut p = POINT::default();
        GetCursorPos(&mut p).ok()?;
        let hit = WindowFromPoint(p);
        let in_rect = p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom;
        let inside = inside_at(in_rect, IsWindowVisible(h).as_bool(), hit == h || IsChild(h, hit).as_bool(), by_rect,
            PASSTHROUGH.load(Ordering::Relaxed));
        let dpi = GetDpiForWindow(h);
        let scale = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };
        let down = |vk: VIRTUAL_KEY| (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0;
        Some(Sample {
            inside,
            x: ((p.x - r.left) as f64 / scale).round(),
            y: ((p.y - r.top) as f64 / scale).round(),
            sx: p.x,
            sy: p.y,
            left: down(VK_LBUTTON),
            right: down(VK_RBUTTON),
        })
    }
}

/// Linux: X11 gives the cursor and the buttons; there is no per-window hit test, so the rectangle decides
/// (a window above the stage swallows its clicks anyway), plus X's enter/leave where the cursor is not global. The right button opens the context menu,
/// like on Windows; the webview's own `contextmenu` event stays suppressed.
#[cfg(not(windows))]
fn sample(raw: isize, _by_rect: bool) -> Option<Sample> {
    if raw == 0 { return None; }
    let h = super::taskbar::hwnd(raw);
    let r = super::taskbar::rect_of(h)?;
    let visible = super::taskbar::is_visible(h);
    let (sx, sy, left, right) = super::taskbar::raw_pointer()?;
    let scale = super::taskbar::scale_of(h);
    let in_rect = sx >= r.left as f64 && sx < r.right as f64 && sy >= r.top as f64 && sy < r.bottom as f64;
    let through = PASSTHROUGH.load(Ordering::Relaxed) && super::taskbar::pointer_global();
    let inside = inside_at(in_rect, visible, super::taskbar::pointer_on(h), true, through);
    Some(Sample {
        inside,
        x: ((sx - r.left as f64) / scale).round(),
        y: ((sy - r.top as f64) / scale).round(),
        sx: sx as i32,
        sy: sy as i32,
        left,
        right,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64) -> Sample { Sample { inside: true, x, y, sx: x as i32 + 1000, sy: y as i32 + 500, ..Sample::default() } }

    #[test]
    fn enter_move_and_leave() {
        let out = Sample::default();
        assert_eq!(diff(&out, &at(10.0, 20.0)), vec![PointerEvent::Move { x: 10.0, y: 20.0 }]);
        assert!(diff(&at(10.0, 20.0), &at(10.0, 20.0)).is_empty());
        assert_eq!(diff(&at(10.0, 20.0), &at(11.0, 20.0)), vec![PointerEvent::Move { x: 11.0, y: 20.0 }]);
        assert_eq!(diff(&at(11.0, 20.0), &out), vec![PointerEvent::Leave]);
    }

    #[test]
    fn taskbar_click_fires_on_release_not_on_press() {
        let mut d = Dragger::new(4);
        let down = Sample { left: true, ..at(5.0, 5.0) };
        assert!(diff(&at(5.0, 5.0), &down).is_empty(), "the press itself is not a click");
        assert_eq!(release_click(&mut d, &at(5.0, 5.0), &down), None);
        assert_eq!(release_click(&mut d, &down, &down), None, "held");
        assert_eq!(release_click(&mut d, &down, &at(6.0, 5.0)), Some(PointerEvent::Click { x: 6.0, y: 5.0 }));
        // released outside the stage: no click
        assert_eq!(release_click(&mut d, &at(5.0, 5.0), &down), None);
        let out = Sample { inside: false, ..at(5.0, 5.0) };
        assert_eq!(release_click(&mut d, &down, &out), None);
        // pressed outside the stage and released inside: no click
        let outside_down = Sample { inside: false, left: true, ..Sample::default() };
        assert_eq!(release_click(&mut d, &Sample::default(), &outside_down), None);
        assert_eq!(release_click(&mut d, &outside_down, &at(5.0, 5.0)), None);
    }

    #[test]
    fn right_button_is_a_context_click() {
        let r = Sample { right: true, ..at(3.0, 4.0) };
        assert_eq!(diff(&at(3.0, 4.0), &r), vec![PointerEvent::Context { x: 3.0, y: 4.0 }]);
    }

    fn press(s: Sample) -> Sample { Sample { left: true, ..s } }

    #[test]
    fn move_mode_drags_from_the_press_and_reports_the_offset_even_outside_the_window() {
        let mut d = Dragger::new(0);
        assert_eq!(d.step(&at(5.0, 5.0), &press(at(5.0, 5.0))), vec![Drag::Start]);
        let moved = Sample { inside: false, ..press(at(45.0, 9.0)) };
        assert_eq!(d.step(&press(at(5.0, 5.0)), &moved), vec![Drag::Move { dx: 40, dy: 4 }]);
        assert!(d.step(&moved, &moved).is_empty(), "no repeat without movement");
        assert_eq!(d.step(&moved, &Sample { left: false, ..moved }), vec![Drag::End]);
    }

    #[test]
    fn floating_short_click_stays_a_click_and_a_real_move_becomes_a_drag() {
        let mut d = Dragger::new(4);
        assert!(d.step(&at(5.0, 5.0), &press(at(5.0, 5.0))).is_empty());
        assert!(d.step(&press(at(5.0, 5.0)), &press(at(7.0, 6.0))).is_empty(), "3 px or less is not a drag");
        assert_eq!(d.step(&press(at(7.0, 6.0)), &at(7.0, 6.0)), vec![Drag::Click { x: 7.0, y: 6.0 }]);
        assert!(d.step(&at(5.0, 5.0), &press(at(5.0, 5.0))).is_empty());
        assert_eq!(d.step(&press(at(5.0, 5.0)), &press(at(10.0, 5.0))), vec![Drag::Start, Drag::Move { dx: 5, dy: 0 }]);
        assert_eq!(d.step(&press(at(10.0, 5.0)), &at(10.0, 5.0)), vec![Drag::End]);
    }

    #[test]
    fn a_press_outside_the_window_starts_nothing() {
        let mut d = Dragger::new(0);
        let out = Sample { inside: false, ..at(5.0, 5.0) };
        assert!(d.step(&out, &press(out)).is_empty());
        assert!(d.step(&press(out), &press(at(9.0, 5.0))).is_empty());
    }

    #[test]
    fn inside_means_visible_and_really_under_the_cursor() {
        assert!(inside_at(true, true, true, false, false));
        assert!(!inside_at(true, false, true, true, false), "hidden (fullscreen game): not ours");
        assert!(!inside_at(true, true, false, true, false), "covered by the menu or another window");
        assert!(inside_at(true, true, false, true, true), "passing clicks through: tracked by the rectangle, presses masked");
        assert!(!inside_at(false, true, true, true, true));
    }

    #[test]
    fn the_enter_that_started_move_mode_does_not_commit_it() {
        let mut k = Keys::default();
        assert_eq!(k.step(false, (true, false)), (false, false));
        assert_eq!(k.step(true, (true, false)), (false, false), "Enter still held from the button or menu");
        assert_eq!(k.step(true, (false, false)), (false, false));
        assert_eq!(k.step(true, (true, false)), (true, false));
        assert_eq!(k.step(true, (true, true)), (false, true));
        assert_eq!(k.step(false, (false, false)), (false, false));
    }

    #[test]
    fn serializes_for_the_ui() {
        assert_eq!(serde_json::to_string(&PointerEvent::Leave).unwrap(), r#"{"kind":"leave"}"#);
        assert_eq!(serde_json::to_string(&PointerEvent::Move { x: 1.0, y: 2.0 }).unwrap(), r#"{"kind":"move","x":1.0,"y":2.0}"#);
    }
}
