//! Computer use: what the agent loop, `hands` and `device` promise each other.
//!
//! Draft v0. The architecture meeting (plan, section 10) freezes these.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// What `act` does to a control from the outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ElementOp {
    Click,
    DoubleClick,
    RightClick,
    Type,
    Scroll,
}

/// One thing the agent does. Every gesture is bounded: modifiers are released
/// inside the gesture, drags last 50-1000 ms, scrolls are at most 20 notches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "do", rename_all = "snake_case")]
#[ts(export)]
pub enum Action {
    /// Look again without acting.
    Observe,
    /// Work a control by its outline reference (`e12`): no pixels involved.
    Act {
        element: String,
        op: ElementOp,
        text: Option<String>,
    },
    /// By position, for models that see. Coordinates are in the image the model was sent.
    Click {
        x: i32,
        y: i32,
        button: Button,
        count: u8,
    },
    Move {
        x: i32,
        y: i32,
    },
    Scroll {
        x: i32,
        y: i32,
        dx: i32,
        dy: i32,
    },
    Drag {
        from: Point,
        to: Point,
        ms: u32,
    },
    /// A key or chord, e.g. `ctrl+s`.
    Key {
        keys: String,
    },
    /// Refused whole if any character can't be typed.
    TypeText {
        text: String,
    },
    /// The whole window's text, including parts scrolled out of view.
    Read,
    Wait {
        ms: u32,
    },
}

/// The window an agent was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub app: String,
}

/// One control in the accessibility outline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Element {
    /// Short reference the model uses, e.g. `e12`. Valid for this observation only.
    pub id: String,
    /// `button`, `switch`, `text_field`, ...
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub bounds: Rect,
    /// What `act` can do with it, e.g. `press`, `toggle`, `set_value`.
    pub actions: Vec<String>,
}

/// What the agent sees after every action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Observation {
    /// Single-use, expires after about 10 s. Acting needs the newest one.
    pub token: String,
    pub window: WindowInfo,
    /// The size of the image sent to the model (= its coordinate space).
    pub size: Size,
    /// A screenshot (URL or data URL), only for models that can see.
    pub image: Option<String>,
    pub outline: Option<Vec<Element>>,
}

/// Why `device` refused an action. A refusal always comes with a fresh observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Refusal {
    /// The token is old or used: look again first.
    StaleToken,
    /// The window moved, resized or changed title since the observation.
    WindowChanged,
    /// The same action with no visible change last time.
    NoVisibleProgress,
    /// The person is using this app right now.
    AppInUse,
    /// Another agent's gesture (a drag) is in progress in this app.
    Busy,
    OutOfBounds,
    BudgetSpent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ActionResult {
    /// The input was delivered; it says nothing about whether the app took it.
    pub ok: bool,
    pub refused: Option<Refusal>,
    /// Always set, even after a failure.
    pub observation: Observation,
    /// True until an observation shows the action worked.
    pub verify_required: bool,
}
