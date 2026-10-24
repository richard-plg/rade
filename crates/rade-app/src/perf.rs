//! Timings behind the M40 budgets (R111-c, R111-d), read by
//! `debug.ui {"op":"perf"}` (test mode): main-loop durations recorded by
//! name (the Navigator's sync), the time from an action to the first frame
//! painted with its content (the Usage page, the `+` menu), and the frames of
//! a scripted end-to-end scroll of the Navigator.

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// At most this many samples per name (the oldest go).
const KEEP: usize = 4096;

thread_local! {
    static SAMPLES: RefCell<BTreeMap<String, Vec<f64>>> = RefCell::default();
    static MARKS: RefCell<HashMap<String, Instant>> = RefCell::default();
    static SCROLLING: Cell<bool> = const { Cell::new(false) };
}

fn push(name: &str, ms: f64) {
    SAMPLES.with(|s| {
        let mut s = s.borrow_mut();
        let v = s.entry(name.to_string()).or_default();
        if v.len() == KEEP {
            v.remove(0);
        }
        v.push(ms);
    });
}

/// One main-loop duration of `name`; `changed` = it changed what is shown
/// (also kept as `<name>:changed`).
pub fn record(name: &str, d: Duration, changed: bool) {
    let ms = d.as_secs_f64() * 1000.0;
    push(name, ms);
    if changed {
        push(&format!("{name}:changed"), ms);
    }
}

/// Start timing `name` (an action that will draw something).
pub fn start(name: &str) {
    MARKS.with(|m| m.borrow_mut().insert(name.to_string(), Instant::now()));
}

/// `name`'s content is in place: the first frame painted after this ends
/// the measurement started by [`start`] (`<name>` in ms). No-op without a
/// pending start.
pub fn ready(name: &str, widget: &impl IsA<gtk::Widget>) {
    let Some(at) = MARKS.with(|m| m.borrow_mut().remove(name)) else {
        return;
    };
    let Some(clock) = widget.frame_clock() else {
        push(name, at.elapsed().as_secs_f64() * 1000.0);
        return;
    };
    let name = name.to_string();
    // The frame's own work (layout → paint end) is kept as `<name>:frame`.
    let begun: Rc<Cell<Option<Instant>>> = Rc::default();
    let b2 = begun.clone();
    let ids: Rc<RefCell<Vec<glib::SignalHandlerId>>> = Rc::default();
    let ids2 = ids.clone();
    let u = clock.connect_layout(move |_| {
        if b2.get().is_none() {
            b2.set(Some(Instant::now()));
        }
    });
    let h = clock.connect_after_paint(move |c| {
        push(&name, at.elapsed().as_secs_f64() * 1000.0);
        if let Some(b) = begun.get() {
            push(&format!("{name}:frame"), b.elapsed().as_secs_f64() * 1000.0);
        }
        for h in ids2.borrow_mut().drain(..) {
            c.disconnect(h);
        }
    });
    ids.borrow_mut().extend([u, h]);
    widget.queue_draw();
}

/// Each frame's own work (layout → paint end) of the next `ms` ms, kept as
/// `<name>:anim` (R117.8: an animation's frames).
pub fn frames(name: &str, widget: &impl IsA<gtk::Widget>, ms: u64) {
    let Some(clock) = widget.frame_clock() else {
        return;
    };
    let until = Instant::now() + std::time::Duration::from_millis(ms);
    let key = format!("{name}:anim");
    let begun: Rc<Cell<Option<Instant>>> = Rc::default();
    let b2 = begun.clone();
    let ids: Rc<RefCell<Vec<glib::SignalHandlerId>>> = Rc::default();
    let ids2 = ids.clone();
    let u = clock.connect_layout(move |_| {
        if b2.get().is_none() {
            b2.set(Some(Instant::now()));
        }
    });
    let h = clock.connect_after_paint(move |c| {
        if let Some(b) = begun.take() {
            push(&key, b.elapsed().as_secs_f64() * 1000.0);
        }
        if Instant::now() >= until {
            for h in ids2.borrow_mut().drain(..) {
                c.disconnect(h);
            }
        }
    });
    ids.borrow_mut().extend([u, h]);
    widget.queue_draw();
}

/// Scroll `sw` from the top to the end over `frames` frames, one step per
/// frame, recording each frame's own work (`<name>:work`: frame clock
/// update → paint end) and the interval between frames (`<name>:interval`).
pub fn scroll(name: &str, sw: &gtk::ScrolledWindow, frames: u32) -> Result<(), String> {
    if SCROLLING.with(Cell::get) {
        return Err("a scroll is running".into());
    }
    let clock = sw.frame_clock().ok_or("the list is not shown")?;
    let adj = sw.vadjustment();
    let span = adj.upper() - adj.page_size();
    if span <= 0.0 {
        return Err(format!("nothing to scroll (upper {})", adj.upper()));
    }
    adj.set_value(0.0);
    SCROLLING.with(|s| s.set(true));
    let started: Rc<Cell<Option<Instant>>> = Rc::default();
    let last: Rc<Cell<Option<i64>>> = Rc::default();
    let (n1, n2) = (format!("{name}:work"), format!("{name}:interval"));
    let s1 = started.clone();
    let update = clock.connect_update(move |_| s1.set(Some(Instant::now())));
    let paint = clock.connect_after_paint(move |c| {
        if let Some(t) = started.take() {
            push(&n1, t.elapsed().as_secs_f64() * 1000.0);
        }
        let now = c.frame_time();
        if let Some(prev) = last.replace(Some(now)) {
            push(&n2, (now - prev) as f64 / 1000.0);
        }
    });
    let step = span / frames as f64;
    let left = Cell::new(frames);
    let handlers = RefCell::new(Some((update, paint)));
    sw.add_tick_callback(move |w, c| {
        let adj = w.vadjustment();
        if left.get() == 0 {
            if let Some((u, p)) = handlers.borrow_mut().take() {
                c.disconnect(u);
                c.disconnect(p);
            }
            SCROLLING.with(|s| s.set(false));
            return glib::ControlFlow::Break;
        }
        left.set(left.get() - 1);
        adj.set_value((adj.value() + step).min(adj.upper() - adj.page_size()));
        glib::ControlFlow::Continue
    });
    Ok(())
}

pub fn scrolling() -> bool {
    SCROLLING.with(Cell::get)
}

/// `p`-th percentile (nearest rank) of `v`.
fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let i = ((p / 100.0 * s.len() as f64).ceil() as usize).clamp(1, s.len()) - 1;
    s[i]
}

/// Every name's count, p50, p95 and max (ms, 3 decimals).
pub fn summary() -> Value {
    let r = |x: f64| (x * 1000.0).round() / 1000.0;
    SAMPLES.with(|s| {
        let mut out = serde_json::Map::new();
        for (k, v) in s.borrow().iter() {
            out.insert(
                k.clone(),
                json!({"n": v.len(), "p50": r(pct(v, 50.0)), "p95": r(pct(v, 95.0)),
                       "max": r(v.iter().copied().fold(0.0, f64::max))}),
            );
        }
        json!({"samples": out, "scrolling": scrolling()})
    })
}

/// Forget every sample (`name` = only that one and its `:` variants).
pub fn reset(name: Option<&str>) {
    SAMPLES.with(|s| match name {
        None => s.borrow_mut().clear(),
        Some(n) => s
            .borrow_mut()
            .retain(|k, _| k != n && !k.starts_with(&format!("{n}:"))),
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn nearest_rank_percentiles() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(super::pct(&v, 95.0), 95.0);
        assert_eq!(super::pct(&v, 50.0), 50.0);
        assert_eq!(super::pct(&[3.0], 95.0), 3.0);
        assert_eq!(super::pct(&[], 95.0), 0.0);
    }
}
