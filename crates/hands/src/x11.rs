//! A whole X11 desktop as [`Hands`]: the Docker Xvfb private desktop, or any
//! display you point it at. Screenshots come from the root window, input goes
//! through the XTest extension, so it works with no window manager and no
//! window of its own.
//!
//! The rules of the contract hold here as in the fake: one single-use token per
//! observation, a stale one refused with a fresh look, clicks outside the image
//! refused and never clamped, and every action followed by a new screenshot.
//! Coordinates the model sends are in the image it saw; `scale` maps them to
//! the screen.

use std::{
    io::Cursor,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use async_trait::async_trait;
use base64::Engine;
use image::{ImageFormat, RgbImage, imageops::FilterType};
use proto::{Action, ActionResult, Button, Observation, Point, Refusal, Size, WindowInfo};
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::{
        xproto::{self, ConnectionExt as _, ImageFormat as XImageFormat, ImageOrder, Keycode},
        xtest::{self, ConnectionExt as _},
    },
    rust_connection::RustConnection,
};

use crate::{Hands, needs_verify, scale};

/// The long edge the screenshot is shrunk to before it goes to a model.
pub const DEFAULT_MAX_EDGE: u32 = 1280;
/// How long the desktop gets to repaint after an action before the next look.
const SETTLE: Duration = Duration::from_millis(150);
const MAX_SCROLL_NOTCHES: i32 = 20;
const MAX_WAIT: Duration = Duration::from_secs(10);

pub struct Desktop {
    conn: Arc<RustConnection>,
    root: xproto::Window,
    screen: Size,
    depth: u8,
    lsb_first: bool,
    bytes_per_pixel: usize,
    keyboard: Keyboard,
    max_edge: u32,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    seen: u32,
    token: Option<String>,
    /// The size of the last image sent, so clicks map back to the screen.
    image: Option<Size>,
}

impl Desktop {
    /// Connects to `display` (e.g. `:99`) and checks it can take input.
    pub fn connect(display: &str) -> anyhow::Result<Self> {
        Self::connect_with(display, DEFAULT_MAX_EDGE)
    }

    pub fn connect_with(display: &str, max_edge: u32) -> anyhow::Result<Self> {
        let (conn, screen_num) = x11rb::connect(Some(display))
            .with_context(|| format!("can't open X display {display}"))?;
        conn.extension_information(xtest::X11_EXTENSION_NAME)?
            .ok_or_else(|| {
                anyhow!("display {display} has no XTest extension: it can't take input")
            })?;
        let setup = conn.setup();
        let screen = &setup.roots[screen_num];
        let depth = screen.root_depth;
        let format = setup
            .pixmap_formats
            .iter()
            .find(|format| format.depth == depth)
            .ok_or_else(|| anyhow!("no pixmap format for depth {depth}"))?;
        if format.bits_per_pixel != 24 && format.bits_per_pixel != 32 {
            bail!(
                "display {display} has {} bits per pixel; only 24 and 32 are read",
                format.bits_per_pixel
            );
        }
        let keyboard = Keyboard::load(&conn)?;
        Ok(Self {
            root: screen.root,
            screen: Size {
                width: u32::from(screen.width_in_pixels),
                height: u32::from(screen.height_in_pixels),
            },
            depth,
            lsb_first: setup.image_byte_order == ImageOrder::LSB_FIRST,
            bytes_per_pixel: usize::from(format.bits_per_pixel / 8),
            keyboard,
            max_edge,
            conn: Arc::new(conn),
            state: Mutex::default(),
        })
    }

    pub fn screen(&self) -> Size {
        self.screen
    }

    /// The screen as RGB.
    fn capture(&self) -> anyhow::Result<RgbImage> {
        let (width, height) = (self.screen.width, self.screen.height);
        let reply = self
            .conn
            .get_image(
                XImageFormat::Z_PIXMAP,
                self.root,
                0,
                0,
                width as u16,
                height as u16,
                !0,
            )?
            .reply()
            .context("can't read the screen")?;
        if reply.depth != self.depth {
            bail!("the screen came back at depth {}", reply.depth);
        }
        let data = reply.data;
        let bpp = self.bytes_per_pixel;
        let stride = data.len() / height as usize;
        let mut image = RgbImage::new(width, height);
        for (y, row) in image.rows_mut().enumerate() {
            let line = &data[y * stride..];
            for (x, pixel) in row.enumerate() {
                let p = &line[x * bpp..x * bpp + bpp];
                // Depth 24 is stored as B, G, R (then padding) on LSB-first
                // servers, and padding, R, G, B on MSB-first ones.
                let (r, g, b) = match (self.lsb_first, bpp) {
                    (true, _) => (p[2], p[1], p[0]),
                    (false, 4) => (p[1], p[2], p[3]),
                    (false, _) => (p[0], p[1], p[2]),
                };
                *pixel = image::Rgb([r, g, b]);
            }
        }
        Ok(image)
    }

    fn observe_now(&self, state: &mut State) -> anyhow::Result<Observation> {
        let full = self.capture()?;
        let size = scale::fit(self.screen, self.max_edge);
        let image = if size == self.screen {
            full
        } else {
            image::imageops::resize(&full, size.width, size.height, FilterType::Triangle)
        };
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, ImageFormat::Png)
            .context("can't encode the screenshot")?;
        let data = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
        state.seen += 1;
        let token = format!("x{}", state.seen);
        state.token = Some(token.clone());
        state.image = Some(size);
        Ok(Observation {
            token,
            window: WindowInfo {
                id: format!("root-{}", self.root),
                title: "Desktop".into(),
                app: "x11".into(),
            },
            size,
            image: Some(format!("data:image/png;base64,{data}")),
            outline: None,
        })
    }

    /// A point in the model's image, on the screen.
    fn on_screen(&self, image: Size, x: i32, y: i32) -> Result<Point, Refusal> {
        scale::image_to_screen(Point { x, y }, image, self.screen).ok_or(Refusal::OutOfBounds)
    }

    fn fake(&self, kind: u8, detail: u8, at: Option<Point>) -> anyhow::Result<()> {
        let (x, y) = match at {
            Some(point) => (point.x as i16, point.y as i16),
            None => (0, 0),
        };
        self.conn
            .xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, self.root, x, y, 0)?;
        Ok(())
    }

    fn motion(&self, to: Point) -> anyhow::Result<()> {
        self.fake(xproto::MOTION_NOTIFY_EVENT, 0, Some(to))
    }

    fn button(&self, button: u8, at: Point) -> anyhow::Result<()> {
        self.fake(xproto::BUTTON_PRESS_EVENT, button, Some(at))?;
        self.fake(xproto::BUTTON_RELEASE_EVENT, button, Some(at))
    }

    fn key(&self, keycode: Keycode, press: bool) -> anyhow::Result<()> {
        let kind = if press {
            xproto::KEY_PRESS_EVENT
        } else {
            xproto::KEY_RELEASE_EVENT
        };
        self.fake(kind, keycode, None)
    }

    /// Presses `keys` with `modifiers` held, releasing everything in reverse.
    fn chord(&self, modifiers: &[Keycode], keys: &[Keycode]) -> anyhow::Result<()> {
        for &modifier in modifiers {
            self.key(modifier, true)?;
        }
        for &key in keys {
            self.key(key, true)?;
            self.key(key, false)?;
        }
        for &modifier in modifiers.iter().rev() {
            self.key(modifier, false)?;
        }
        Ok(())
    }

    /// `Ok(delivered)`, or the refusal. Nothing is sent for a refusal.
    async fn apply(&self, image: Size, action: &Action) -> anyhow::Result<Result<bool, Refusal>> {
        let ok = Ok(true);
        match action {
            Action::Observe => Ok(ok),
            // No outline on a bare desktop: there is no `e12` to act on.
            Action::Act { .. } => Ok(Ok(false)),
            // The screen has no text to read without an accessibility tree.
            Action::Read => Ok(Ok(false)),
            Action::Wait { ms } => {
                tokio::time::sleep(Duration::from_millis(u64::from(*ms)).min(MAX_WAIT)).await;
                Ok(ok)
            }
            Action::Move { x, y } => {
                let at = match self.on_screen(image, *x, *y) {
                    Ok(at) => at,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                self.motion(at)?;
                self.conn.flush()?;
                Ok(ok)
            }
            Action::Click {
                x,
                y,
                button,
                count,
            } => {
                let at = match self.on_screen(image, *x, *y) {
                    Ok(at) => at,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                let button = match button {
                    Button::Left => 1,
                    Button::Middle => 2,
                    Button::Right => 3,
                };
                self.motion(at)?;
                for _ in 0..(*count).clamp(1, 2) {
                    self.button(button, at)?;
                }
                self.conn.flush()?;
                Ok(ok)
            }
            Action::Scroll { x, y, dx, dy } => {
                let at = match self.on_screen(image, *x, *y) {
                    Ok(at) => at,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                self.motion(at)?;
                // Buttons 4/5 scroll up/down, 6/7 left/right, one notch each.
                let notches = |delta: i32, back: u8, forth: u8| -> (u8, i32) {
                    if delta < 0 {
                        (back, delta.abs().min(MAX_SCROLL_NOTCHES))
                    } else {
                        (forth, delta.min(MAX_SCROLL_NOTCHES))
                    }
                };
                for (button, n) in [notches(*dy, 4, 5), notches(*dx, 6, 7)] {
                    for _ in 0..n {
                        self.button(button, at)?;
                    }
                }
                self.conn.flush()?;
                Ok(ok)
            }
            Action::Drag { from, to, ms } => {
                let (from, to) = match (
                    self.on_screen(image, from.x, from.y),
                    self.on_screen(image, to.x, to.y),
                ) {
                    (Ok(from), Ok(to)) => (from, to),
                    _ => return Ok(Err(Refusal::OutOfBounds)),
                };
                let total = Duration::from_millis(u64::from(*ms).clamp(50, 1000));
                let steps: i32 = 10;
                self.motion(from)?;
                self.fake(xproto::BUTTON_PRESS_EVENT, 1, Some(from))?;
                self.conn.flush()?;
                for step in 1..=steps {
                    tokio::time::sleep(total / steps as u32).await;
                    let at = Point {
                        x: from.x + (to.x - from.x) * step / steps,
                        y: from.y + (to.y - from.y) * step / steps,
                    };
                    self.motion(at)?;
                    self.conn.flush()?;
                }
                self.fake(xproto::BUTTON_RELEASE_EVENT, 1, Some(to))?;
                self.conn.flush()?;
                Ok(ok)
            }
            Action::Key { keys } => {
                let Some((modifiers, key)) = self.keyboard.chord(keys) else {
                    return Ok(Ok(false));
                };
                self.chord(&modifiers, &[key])?;
                self.conn.flush()?;
                Ok(ok)
            }
            Action::TypeText { text } => {
                // Refused whole if any character can't be typed.
                let Some(strokes) = self.keyboard.strokes(text) else {
                    return Ok(Ok(false));
                };
                for (shift, keycode) in strokes {
                    let modifiers: Vec<Keycode> = shift.into_iter().collect();
                    self.chord(&modifiers, &[keycode])?;
                }
                self.conn.flush()?;
                Ok(ok)
            }
        }
    }
}

#[async_trait]
impl Hands for Desktop {
    async fn observe(&self) -> anyhow::Result<Observation> {
        let mut state = self.state.lock().unwrap();
        self.observe_now(&mut state)
    }

    async fn act(&self, token: &str, action: &Action) -> anyhow::Result<ActionResult> {
        // Single use: whatever happens next, this token is spent.
        let image = {
            let mut state = self.state.lock().unwrap();
            let fresh = state.token.take().is_some_and(|newest| newest == token);
            fresh.then_some(state.image).flatten()
        };
        let outcome = match image {
            Some(image) => self.apply(image, action).await?,
            None => Err(Refusal::StaleToken),
        };
        if matches!(outcome, Ok(true)) && needs_verify(action) {
            tokio::time::sleep(SETTLE).await;
        }
        let observation = {
            let mut state = self.state.lock().unwrap();
            self.observe_now(&mut state)?
        };
        Ok(match outcome {
            Ok(ok) => ActionResult {
                ok,
                refused: None,
                observation,
                verify_required: ok && needs_verify(action),
            },
            Err(refusal) => ActionResult {
                ok: false,
                refused: Some(refusal),
                observation,
                verify_required: false,
            },
        })
    }
}

/// The server's keyboard mapping: which keycode (and whether Shift) types a keysym.
struct Keyboard {
    min: Keycode,
    per_keycode: usize,
    keysyms: Vec<u32>,
    shift: Option<Keycode>,
}

// Keysyms from X11/keysymdef.h.
const XK_SHIFT_L: u32 = 0xffe1;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_ALT_L: u32 = 0xffe9;
const XK_SUPER_L: u32 = 0xffeb;

impl Keyboard {
    fn load(conn: &RustConnection) -> anyhow::Result<Self> {
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let reply = conn
            .get_keyboard_mapping(min, max - min + 1)?
            .reply()
            .context("can't read the keyboard mapping")?;
        let mut keyboard = Self {
            min,
            per_keycode: usize::from(reply.keysyms_per_keycode),
            keysyms: reply.keysyms,
            shift: None,
        };
        keyboard.shift = keyboard.find(XK_SHIFT_L).map(|(keycode, _)| keycode);
        Ok(keyboard)
    }

    /// The keycode for `keysym`, and whether it needs Shift.
    fn find(&self, keysym: u32) -> Option<(Keycode, bool)> {
        let per = self.per_keycode;
        for (index, group) in self.keysyms.chunks(per).enumerate() {
            let keycode = self.min + index as u8;
            if group.first() == Some(&keysym) {
                return Some((keycode, false));
            }
            if per > 1 && group.get(1) == Some(&keysym) {
                return Some((keycode, true));
            }
        }
        None
    }

    /// Every stroke of `text`: (Shift if needed, keycode). `None` if a character can't be typed.
    fn strokes(&self, text: &str) -> Option<Vec<(Option<Keycode>, Keycode)>> {
        text.chars()
            .map(|c| {
                let keysym = match c {
                    '\n' => 0xff0d,
                    '\t' => 0xff09,
                    // Latin-1 keysyms are the character codes.
                    ' '..='~' | '\u{a0}'..='\u{ff}' => c as u32,
                    _ => return None,
                };
                let (keycode, shift) = self.find(keysym)?;
                let shift = if shift { Some(self.shift?) } else { None };
                Some((shift, keycode))
            })
            .collect()
    }

    /// `ctrl+shift+s`: the modifiers' keycodes and the key's.
    fn chord(&self, keys: &str) -> Option<(Vec<Keycode>, Keycode)> {
        let parts: Vec<&str> = keys.split('+').map(str::trim).collect();
        let (key, modifiers) = parts.split_last()?;
        let mut held = Vec::new();
        for modifier in modifiers {
            let keysym = match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => XK_CONTROL_L,
                "shift" => XK_SHIFT_L,
                "alt" | "option" => XK_ALT_L,
                "super" | "meta" | "cmd" | "win" => XK_SUPER_L,
                _ => return None,
            };
            held.push(self.find(keysym)?.0);
        }
        let keysym = named_keysym(key)?;
        let (keycode, shift) = self.find(keysym)?;
        if shift && !held.contains(&self.shift?) {
            held.insert(0, self.shift?);
        }
        Some((held, keycode))
    }
}

/// A key's name in a chord, as a keysym.
fn named_keysym(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let keysym = match lower.as_str() {
        "enter" | "return" => 0xff0d,
        "tab" => 0xff09,
        "esc" | "escape" => 0xff1b,
        "space" => 0x20,
        "backspace" => 0xff08,
        "delete" | "del" => 0xffff,
        "home" => 0xff50,
        "end" => 0xff57,
        "pageup" | "page_up" => 0xff55,
        "pagedown" | "page_down" => 0xff56,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        _ => {
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                // A single printable character, as typed: `s`, `/`, `1`.
                (Some(c), None) if (' '..='~').contains(&c) => c as u32,
                _ => {
                    // f1 .. f12
                    let n: u32 = lower.strip_prefix('f')?.parse().ok()?;
                    (1..=12).contains(&n).then_some(0xffbe + n - 1)?
                }
            }
        }
    };
    Some(keysym)
}

#[cfg(test)]
mod tests {
    //! On a real Xvfb server, when one can be started; otherwise the tests say
    //! so and pass. CI installs `xvfb` so they run there.

    use std::{
        process::{Child, Command, Stdio},
        time::Instant,
    };

    use image::GenericImageView;
    use x11rb::protocol::{
        Event,
        xproto::{CreateWindowAux, EventMask, InputFocus, WindowClass},
    };

    use super::*;

    /// An Xvfb server on a display of its own, stopped when dropped.
    struct Xvfb {
        child: Child,
        display: String,
        /// Held open: an X server resets itself when its last client leaves.
        _probe: Option<RustConnection>,
    }

    impl Xvfb {
        fn start() -> Option<Self> {
            // Tests run in parallel, so each takes the next display number.
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(90);
            for _ in 0..10 {
                let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let display = format!(":{n}");
                if std::path::Path::new(&format!("/tmp/.X11-unix/X{n}")).exists() {
                    continue;
                }
                let child = Command::new("Xvfb")
                    .args([
                        &display,
                        "-screen",
                        "0",
                        "1600x1000x24",
                        "-nolisten",
                        "tcp",
                        "-noreset",
                    ])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .ok()?;
                let mut server = Self {
                    child,
                    display,
                    _probe: None,
                };
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    if let Ok((conn, _)) = x11rb::connect(Some(&server.display)) {
                        server._probe = Some(conn);
                        return Some(server);
                    }
                    if server.child.try_wait().ok().flatten().is_some() {
                        break; // that display was taken after all
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            None
        }
    }

    impl Drop for Xvfb {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            // A killed server leaves its own socket and lock behind.
            let n = &self.display[1..];
            let _ = std::fs::remove_file(format!("/tmp/.X11-unix/X{n}"));
            let _ = std::fs::remove_file(format!("/tmp/.X{n}-lock"));
        }
    }

    /// A red window over the whole screen that records the input it gets.
    struct Target {
        conn: RustConnection,
    }

    impl Target {
        fn open(display: &str) -> Self {
            let (conn, screen_num) = x11rb::connect(Some(display)).unwrap();
            let screen = &conn.setup().roots[screen_num];
            let window = conn.generate_id().unwrap();
            conn.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                screen.root,
                0,
                0,
                screen.width_in_pixels,
                screen.height_in_pixels,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new()
                    .background_pixel(0xff0000)
                    .event_mask(
                        EventMask::EXPOSURE
                            | EventMask::BUTTON_PRESS
                            | EventMask::BUTTON_RELEASE
                            | EventMask::KEY_PRESS
                            | EventMask::POINTER_MOTION,
                    ),
            )
            .unwrap();
            conn.map_window(window).unwrap();
            conn.set_input_focus(InputFocus::POINTER_ROOT, window, x11rb::CURRENT_TIME)
                .unwrap();
            conn.flush().unwrap();
            let target = Self { conn };
            // Painted once Expose arrives.
            target.wait_for(|event| matches!(event, Event::Expose(_)));
            target
        }

        /// Events until `done` matches one, or two seconds pass.
        fn wait_for(&self, done: impl Fn(&Event) -> bool) -> Vec<Event> {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut events = Vec::new();
            while Instant::now() < deadline {
                match self.conn.poll_for_event().unwrap() {
                    Some(event) => {
                        let finished = done(&event);
                        events.push(event);
                        if finished {
                            return events;
                        }
                    }
                    None => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            events
        }
    }

    fn decode(observation: &Observation) -> image::DynamicImage {
        let data = observation
            .image
            .as_deref()
            .unwrap()
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        image::load_from_memory(&bytes).unwrap()
    }

    macro_rules! xvfb_or_skip {
        () => {
            match Xvfb::start() {
                Some(server) => server,
                None => {
                    eprintln!("skipped: no Xvfb on this machine");
                    return;
                }
            }
        };
    }

    #[tokio::test]
    async fn screenshots_are_shrunk_pngs_of_the_screen() {
        let server = xvfb_or_skip!();
        let _target = Target::open(&server.display);
        let desktop = Desktop::connect_with(&server.display, 800).unwrap();
        assert_eq!(
            desktop.screen(),
            Size {
                width: 1600,
                height: 1000
            }
        );
        let observation = desktop.observe().await.unwrap();
        assert_eq!(
            observation.size,
            Size {
                width: 800,
                height: 500
            }
        );
        assert!(observation.outline.is_none());
        let image = decode(&observation);
        assert_eq!(image.dimensions(), (800, 500));
        assert_eq!(
            image.get_pixel(400, 250).0[..3],
            [255, 0, 0],
            "the red window"
        );
    }

    #[tokio::test]
    async fn clicks_land_where_the_model_pointed_in_its_image() {
        let server = xvfb_or_skip!();
        let target = Target::open(&server.display);
        let desktop = Desktop::connect_with(&server.display, 800).unwrap();
        let observation = desktop.observe().await.unwrap();
        // (200, 100) in the half-size image is (400, 200) on the screen.
        let click = Action::Click {
            x: 200,
            y: 100,
            button: Button::Left,
            count: 1,
        };
        let result = desktop.act(&observation.token, &click).await.unwrap();
        assert!(result.ok && result.verify_required && result.refused.is_none());
        let events = target.wait_for(|event| matches!(event, Event::ButtonPress(_)));
        let press = events
            .iter()
            .find_map(|event| match event {
                Event::ButtonPress(press) => Some(press),
                _ => None,
            })
            .expect("the window got the click");
        assert_eq!((press.root_x, press.root_y, press.detail), (400, 200, 1));

        // The token was spent; a second use is refused with a fresh look.
        let again = desktop.act(&observation.token, &click).await.unwrap();
        assert_eq!(again.refused, Some(Refusal::StaleToken));
        assert_ne!(again.observation.token, observation.token);
    }

    #[tokio::test]
    async fn clicks_outside_the_image_are_refused_not_clamped() {
        let server = xvfb_or_skip!();
        let target = Target::open(&server.display);
        let desktop = Desktop::connect_with(&server.display, 800).unwrap();
        let observation = desktop.observe().await.unwrap();
        let click = Action::Click {
            x: 800,
            y: 10,
            button: Button::Left,
            count: 1,
        };
        let result = desktop.act(&observation.token, &click).await.unwrap();
        assert_eq!(result.refused, Some(Refusal::OutOfBounds));
        assert!(!result.ok);
        let events = target.wait_for(|event| matches!(event, Event::ButtonPress(_)));
        assert!(
            !events.iter().any(|e| matches!(e, Event::ButtonPress(_))),
            "nothing was sent"
        );
    }

    #[tokio::test]
    async fn text_and_chords_arrive_as_key_presses() {
        let server = xvfb_or_skip!();
        let target = Target::open(&server.display);
        let desktop = Desktop::connect(&server.display).unwrap();
        let keyboard = &desktop.keyboard;
        let (h, _) = keyboard.find('h' as u32).unwrap();
        let (bang, shifted) = keyboard.find('!' as u32).unwrap();
        assert!(shifted, "`!` needs Shift on a US keyboard");
        let shift = keyboard.shift.unwrap();

        let observation = desktop.observe().await.unwrap();
        let typed = Action::TypeText { text: "h!".into() };
        let result = desktop.act(&observation.token, &typed).await.unwrap();
        assert!(result.ok);
        let presses: Vec<Keycode> = target
            .wait_for(|event| matches!(event, Event::KeyPress(press) if press.detail == bang))
            .into_iter()
            .filter_map(|event| match event {
                Event::KeyPress(press) => Some(press.detail),
                _ => None,
            })
            .collect();
        assert_eq!(presses, [h, shift, bang]);

        // A chord holds the modifier around the key.
        let (s, _) = keyboard.find('s' as u32).unwrap();
        let (ctrl, _) = keyboard.find(XK_CONTROL_L).unwrap();
        let chord = Action::Key {
            keys: "ctrl+s".into(),
        };
        let result = desktop
            .act(&result.observation.token, &chord)
            .await
            .unwrap();
        assert!(result.ok);
        let presses: Vec<Keycode> = target
            .wait_for(|event| matches!(event, Event::KeyPress(press) if press.detail == s))
            .into_iter()
            .filter_map(|event| match event {
                Event::KeyPress(press) => Some(press.detail),
                _ => None,
            })
            .collect();
        assert_eq!(presses, [ctrl, s]);

        // A character the keyboard can't type refuses the whole text.
        let result = desktop
            .act(
                &result.observation.token,
                &Action::TypeText {
                    text: "ok 日本".into(),
                },
            )
            .await
            .unwrap();
        assert!(!result.ok && result.refused.is_none());
    }

    #[tokio::test]
    async fn the_desktop_has_no_outline_to_act_on() {
        let server = xvfb_or_skip!();
        let _target = Target::open(&server.display);
        let desktop = Desktop::connect(&server.display).unwrap();
        let observation = desktop.observe().await.unwrap();
        let act = Action::Act {
            element: "e12".into(),
            op: proto::ElementOp::Click,
            text: None,
        };
        let result = desktop.act(&observation.token, &act).await.unwrap();
        assert!(!result.ok && result.refused.is_none());
    }

    #[test]
    fn chord_names_are_keysyms() {
        assert_eq!(named_keysym("enter"), Some(0xff0d));
        assert_eq!(named_keysym("F12"), Some(0xffc9));
        assert_eq!(named_keysym("f13"), None);
        assert_eq!(named_keysym("s"), Some('s' as u32));
        assert_eq!(named_keysym("nope"), None);
    }
}
