//! Hides the floating navigation pill while the user reads on.
//!
//! Scrolling down hides it; scrolling up, reaching the top or the end of the page, and a tap on
//! the bottom edge bring it back. While the on-screen keyboard is open it stays hidden too.

use std::{cell::Cell, rc::Rc};

use leptos::wasm_bindgen::{JsCast, closure::Closure};
use leptos::{ev, prelude::*};

/// Scrolled down by at least this much since the last event: hide.
const HIDE_STEP: f64 = 6.0;
/// Scrolled up by at least this much: show.
const SHOW_STEP: f64 = 4.0;
/// The first stretch of a page never hides the pill.
const TOP_ZONE: f64 = 48.0;
/// A pointer press this close to the bottom edge shows the pill again.
const EDGE_ZONE: f64 = 32.0;
/// The viewport lost more than this share of its height: the keyboard is open.
const KEYBOARD_SHARE: f64 = 0.75;
/// Screens up to this width count as narrow (as in `extras.css`).
const NARROW: f64 = 1100.0;

/// Next state of the pill after a scroll.
///
/// `delta` is how far the position moved since the last event (positive: down), `pos` the new
/// position and `at_end` whether the end of the scrollable area is reached.
pub fn next_hidden(hidden: bool, delta: f64, pos: f64, at_end: bool) -> bool {
    if pos <= 0.0 || at_end {
        false
    } else if delta >= HIDE_STEP && pos > TOP_ZONE {
        true
    } else if delta <= -SHOW_STEP {
        false
    } else {
        hidden
    }
}

/// Whether the viewport shrank so far that the on-screen keyboard must be open.
pub fn keyboard_open(height: f64, tallest: f64, width: f64) -> bool {
    width <= NARROW && tallest > 0.0 && height < tallest * KEYBOARD_SHARE
}

/// Position, total and visible height of what scrolled.
fn metrics(target: &web_sys::EventTarget) -> Option<(f64, f64, f64)> {
    if let Some(el) = target.dyn_ref::<web_sys::Element>() {
        return Some((
            el.scroll_top() as f64,
            el.scroll_height() as f64,
            el.client_height() as f64,
        ));
    }
    let doc = target.dyn_ref::<web_sys::Document>()?;
    let root = doc.document_element()?;
    let pos = window().scroll_y().unwrap_or(0.0);
    Some((
        pos,
        root.scroll_height() as f64,
        root.client_height() as f64,
    ))
}

/// A scroll inside a sheet or dialog says nothing about the page.
fn in_overlay(target: &web_sys::EventTarget) -> bool {
    target.dyn_ref::<web_sys::Element>().is_some_and(|el| {
        el.closest(".ui-bottomnav-overlay, .ui-dialog-overlay")
            .ok()
            .flatten()
            .is_some()
    })
}

/// `true` while the pill should be out of the way. Call once, inside the component that renders it.
pub fn use_autohide() -> Signal<bool> {
    let scrolled = RwSignal::new(false);
    let keyboard = RwSignal::new(false);

    // Scroll events do not bubble: listen in the capture phase to see every scroller.
    let last: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    let last_target: Rc<std::cell::RefCell<Option<web_sys::EventTarget>>> = Rc::default();
    let on_scroll = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        if in_overlay(&target) {
            return;
        }
        let Some((pos, total, visible)) = metrics(&target) else {
            return;
        };
        let same = last_target.borrow().as_ref().is_some_and(|t| t == &target);
        let delta = if same { pos - last.get() } else { 0.0 };
        *last_target.borrow_mut() = Some(target);
        last.set(pos);
        let at_end = pos + visible >= total - 2.0;
        scrolled.update(|hidden| *hidden = next_hidden(*hidden, delta, pos, at_end));
    });
    let document = document();
    let _ = document.add_event_listener_with_callback_and_bool(
        "scroll",
        on_scroll.as_ref().unchecked_ref(),
        true,
    );
    let listener = StoredValue::new_local(on_scroll);
    on_cleanup(move || {
        listener.with_value(|f| {
            let _ = leptos::prelude::document().remove_event_listener_with_callback_and_bool(
                "scroll",
                f.as_ref().unchecked_ref(),
                true,
            );
        });
    });

    // A tap at the bottom edge calls the pill back.
    let press = window_event_listener(ev::pointerdown, move |event| {
        let height = window()
            .inner_height()
            .ok()
            .and_then(|h| h.as_f64())
            .unwrap_or(0.0);
        if scrolled.get_untracked() && f64::from(event.client_y()) >= height - EDGE_ZONE {
            scrolled.set(false);
        }
    });
    on_cleanup(move || press.remove());

    // The on-screen keyboard shrinks the viewport on Android; iOS is covered by the CSS rule for
    // a focused input.
    let tallest = Cell::new((0.0_f64, 0.0_f64));
    let resize = window_event_listener(ev::resize, move |_| {
        let win = window();
        let width = win
            .inner_width()
            .ok()
            .and_then(|w| w.as_f64())
            .unwrap_or(0.0);
        let height = win
            .inner_height()
            .ok()
            .and_then(|h| h.as_f64())
            .unwrap_or(0.0);
        let (known_width, known_height) = tallest.get();
        let tall = if (known_width - width).abs() > f64::EPSILON {
            height
        } else {
            known_height.max(height)
        };
        tallest.set((width, tall));
        keyboard.set(keyboard_open(height, tall, width));
    });
    on_cleanup(move || resize.remove());

    Signal::derive(move || scrolled.get() || keyboard.get())
}

#[cfg(test)]
mod tests;
