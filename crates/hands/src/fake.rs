//! A fake Settings window: a sidebar with Appearance (`e3`) and, once that's open,
//! a Dark mode switch (`e12`). It enforces the same rules a real backend does
//! (single-use tokens, out-of-bounds refused, a fresh observation every time), so
//! the agent loop is tested against the contract without a desktop.

use std::sync::Mutex;

use async_trait::async_trait;
use proto::{
    Action, ActionResult, Element, ElementOp, Observation, Rect, Refusal, Size, WindowInfo,
};

use crate::{Hands, needs_verify};

const SIZE: Size = Size {
    width: 1280,
    height: 800,
};

#[derive(Default)]
pub struct FakeSettings {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    appearance_open: bool,
    dark: bool,
    /// Counts observations; the newest one's token is the only one accepted.
    seen: u32,
    token: Option<String>,
}

impl FakeSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// For tests: what an independent check (not the model) sees.
    pub fn is_dark(&self) -> bool {
        self.state.lock().unwrap().dark
    }
}

impl State {
    fn observe(&mut self) -> Observation {
        self.seen += 1;
        let token = format!("t{}", self.seen);
        self.token = Some(token.clone());
        Observation {
            token,
            window: WindowInfo {
                id: "fake-1".into(),
                title: "Settings".into(),
                app: "fake-settings".into(),
            },
            size: SIZE,
            image: None,
            outline: Some(self.outline()),
        }
    }

    fn outline(&self) -> Vec<Element> {
        let button = |id: &str, name: &str, y| Element {
            id: id.into(),
            role: "button".into(),
            name: name.into(),
            value: None,
            bounds: Rect {
                x: 12,
                y,
                width: 236,
                height: 40,
            },
            actions: vec!["press".into()],
        };
        let mut outline = vec![button("e1", "Wi-Fi", 80), button("e3", "Appearance", 160)];
        if self.appearance_open {
            outline.push(Element {
                id: "e12".into(),
                role: "switch".into(),
                name: "Dark mode".into(),
                value: Some(if self.dark { "on" } else { "off" }.into()),
                bounds: Rect {
                    x: 1060,
                    y: 186,
                    width: 70,
                    height: 36,
                },
                actions: vec!["toggle".into()],
            });
        }
        outline
    }

    fn press(&mut self, element: &str) -> bool {
        match element {
            "e1" => {
                self.appearance_open = false;
                true
            }
            "e3" => {
                self.appearance_open = true;
                true
            }
            "e12" if self.appearance_open => {
                self.dark = !self.dark;
                true
            }
            _ => false,
        }
    }

    fn element_at(&self, x: i32, y: i32) -> Option<String> {
        self.outline()
            .into_iter()
            .find(|e| {
                let b = e.bounds;
                x >= b.x && y >= b.y && x < b.x + b.width as i32 && y < b.y + b.height as i32
            })
            .map(|e| e.id)
    }

    /// `Ok(delivered)`, or the refusal.
    fn apply(&mut self, action: &Action) -> Result<bool, Refusal> {
        let inside = |x: i32, y: i32| {
            x >= 0 && y >= 0 && (x as u32) < SIZE.width && (y as u32) < SIZE.height
        };
        match action {
            Action::Observe | Action::Read | Action::Wait { .. } => Ok(true),
            Action::Act {
                element,
                op: ElementOp::Click,
                ..
            } => Ok(self.press(element)),
            Action::Act { .. } => Ok(false),
            Action::Click { x, y, .. } | Action::Move { x, y } | Action::Scroll { x, y, .. }
                if !inside(*x, *y) =>
            {
                Err(Refusal::OutOfBounds)
            }
            Action::Click { x, y, .. } => {
                Ok(self.element_at(*x, *y).is_some_and(|id| self.press(&id)))
            }
            Action::Move { .. } | Action::Scroll { .. } | Action::Key { .. } => Ok(true),
            Action::Drag { from, to, .. } if !inside(from.x, from.y) || !inside(to.x, to.y) => {
                Err(Refusal::OutOfBounds)
            }
            Action::Drag { .. } | Action::TypeText { .. } => Ok(true),
        }
    }
}

#[async_trait]
impl Hands for FakeSettings {
    async fn observe(&self) -> anyhow::Result<Observation> {
        Ok(self.state.lock().unwrap().observe())
    }

    async fn act(&self, token: &str, action: &Action) -> anyhow::Result<ActionResult> {
        let mut state = self.state.lock().unwrap();
        // Single use: whatever happens next, this token is spent.
        let fresh = state.token.take().is_some_and(|newest| newest == token);
        let outcome = if fresh {
            state.apply(action)
        } else {
            Err(Refusal::StaleToken)
        };
        let observation = state.observe();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn press(element: &str) -> Action {
        Action::Act {
            element: element.into(),
            op: ElementOp::Click,
            text: None,
        }
    }

    #[tokio::test]
    async fn turns_on_dark_mode() {
        let hands = FakeSettings::new();
        let first = hands.observe().await.unwrap();
        let opened = hands.act(&first.token, &press("e3")).await.unwrap();
        assert!(opened.ok && opened.verify_required);
        let toggled = hands
            .act(&opened.observation.token, &press("e12"))
            .await
            .unwrap();
        assert!(toggled.ok);
        let switch = toggled.observation.outline.unwrap().pop().unwrap();
        assert_eq!(switch.value.as_deref(), Some("on"));
        assert!(hands.is_dark());
    }

    #[tokio::test]
    async fn stale_tokens_are_refused_with_fresh_evidence() {
        let hands = FakeSettings::new();
        let first = hands.observe().await.unwrap();
        let _second = hands.observe().await.unwrap();
        let result = hands.act(&first.token, &press("e3")).await.unwrap();
        assert_eq!(result.refused, Some(Refusal::StaleToken));
        assert!(!result.ok);
        // A token is single use, even after a refusal.
        let again = hands.act(&first.token, &press("e3")).await.unwrap();
        assert_eq!(again.refused, Some(Refusal::StaleToken));
        let ok = hands
            .act(&again.observation.token, &press("e3"))
            .await
            .unwrap();
        assert!(ok.ok);
    }

    #[tokio::test]
    async fn out_of_bounds_clicks_are_refused() {
        let hands = FakeSettings::new();
        let obs = hands.observe().await.unwrap();
        let click = Action::Click {
            x: 1280,
            y: 10,
            button: proto::Button::Left,
            count: 1,
        };
        let result = hands.act(&obs.token, &click).await.unwrap();
        assert_eq!(result.refused, Some(Refusal::OutOfBounds));
    }
}
