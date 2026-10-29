//! The desktop app (GPUI): start runs and watch them live.
//!
//! It talks to the backend only through the `client` crate (HTTP + WebSocket), so
//! it works the same against its built-in mock, `harness serve --mock`, the real
//! backend, or the club server. Without `HARNESS_URL` it starts the scripted mock
//! inside the app; with it (`cargo dev` sets http://127.0.0.1:7878) it uses that
//! backend.

// No console window behind release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod net;
mod views;

use gpui_kit::component::Theme;
use gpui_kit::*;

fn main() {
    let net = net::Net::start();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        Theme::sync_system_appearance(None, cx);
        cx.set_global(net);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1100.), px(720.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Harness".into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(720.), px(480.))),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| views::Workspace::new(window, cx))
        })
        .expect("failed to open the window");
        cx.activate(true);
    });
}
