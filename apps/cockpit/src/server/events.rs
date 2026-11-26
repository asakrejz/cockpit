//! The event stream of the server (`GET /api/events`).

use cockpit_core::api::ServerEvent;
use web_sys::wasm_bindgen::{JsCast, closure::Closure};

/// Open event stream. On closing (`Drop`) the connection is ended.
pub struct Events {
    source: web_sys::EventSource,
}

impl Drop for Events {
    fn drop(&mut self) {
        self.source.close();
    }
}

impl Events {
    /// Opens `GET /api/events`. After an interruption the browser reconnects itself; then
    /// `on_open` comes again.
    pub fn open(
        on_open: impl Fn() + 'static,
        on_event: impl Fn(ServerEvent) + 'static,
        on_error: impl Fn() + 'static,
    ) -> Option<Self> {
        let source = web_sys::EventSource::new("/api/events").ok()?;
        let open = Closure::<dyn Fn()>::new(on_open);
        let error = Closure::<dyn Fn()>::new(on_error);
        let message =
            Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                let parsed = ev
                    .data()
                    .as_string()
                    .and_then(|text| serde_json::from_str::<ServerEvent>(&text).ok());
                if let Some(event) = parsed {
                    on_event(event);
                }
            });
        source.set_onopen(Some(open.as_ref().unchecked_ref()));
        source.set_onerror(Some(error.as_ref().unchecked_ref()));
        source.set_onmessage(Some(message.as_ref().unchecked_ref()));
        // The callbacks live as long as the page is open.
        open.forget();
        error.forget();
        message.forget();
        Some(Self { source })
    }
}
