//! Memory and loop checks (plan, section 10.4).
//!
//! - **Trail:** only the newest observation goes to the model in full. Once a
//!   newer one arrives, the older one shrinks to one line, so a long run doesn't
//!   resend every outline and screenshot on every turn.
//! - **`note`:** the model keeps what it needs from older observations by
//!   writing it down (see `tools/note.rs`). Notes stay in the conversation, and
//!   the app shows them as the step's note.
//! - **No visible progress:** an action that changed nothing visible isn't sent
//!   again while the window still looks the same. The loop refuses it with
//!   [`Refusal::NoVisibleProgress`] and a fresh observation instead.

use proto::{Action, ActionResult, ContentBlock, Message, Observation, Refusal};

use crate::{Call, find_tool};

/// Where an observation's blocks sit in the conversation. In both places they
/// follow one leading text block (the task, or what the action did).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    /// `messages[i].content`: the first user message.
    Message(usize),
    /// The tool result at `messages[message].content[block]`.
    Result { message: usize, block: usize },
}

/// Keeps track of the one observation the model still sees in full.
#[derive(Debug, Default)]
pub(crate) struct Trail {
    /// The newest full observation, and the line it shrinks to.
    newest: Option<(Slot, String)>,
}

impl Trail {
    /// `observation` now sits in full at `slot`. The one before it shrinks to
    /// its trail line.
    pub(crate) fn push(
        &mut self,
        messages: &mut [Message],
        slot: Slot,
        observation: &Observation,
        vision: bool,
    ) {
        if let Some((older, line)) = self.newest.take()
            && let Some(content) = blocks(messages, older)
        {
            content.truncate(1);
            content.push(ContentBlock::text(line));
        }
        self.newest = Some((slot, trail_line(observation, vision)));
    }
}

fn blocks(messages: &mut [Message], slot: Slot) -> Option<&mut Vec<ContentBlock>> {
    match slot {
        Slot::Message(i) => messages.get_mut(i).map(|message| &mut message.content),
        Slot::Result { message, block } => {
            match messages.get_mut(message)?.content.get_mut(block)? {
                ContentBlock::ToolResult { content, .. } => Some(content),
                _ => None,
            }
        }
    }
}

/// An older observation, in one line.
pub(crate) fn trail_line(observation: &Observation, vision: bool) -> String {
    let controls = match &observation.outline {
        Some(outline) if outline.len() == 1 => "1 control".to_owned(),
        Some(outline) => format!("{} controls", outline.len()),
        None => "no outline".to_owned(),
    };
    let screenshot = if vision && observation.image.is_some() {
        ", screenshot dropped"
    } else {
        ""
    };
    format!(
        "(Older observation, shrunk to one line: window \"{}\" ({}), {}x{} px, {controls}{screenshot}.)",
        observation.window.title,
        observation.window.app,
        observation.size.width,
        observation.size.height,
    )
}

/// What a step's note says: the model's text, then whatever it wrote down with
/// `note` in the same turn.
pub(crate) fn step_note(content: &[ContentBlock]) -> String {
    let mut lines = vec![crate::texts(content)];
    for block in content {
        if let ContentBlock::ToolCall { name, args, .. } = block
            && let Some(Ok(Call::Note { text })) = find_tool(name).map(|tool| tool.parse(args))
        {
            lines.push(text);
        }
    }
    lines.retain(|line| !line.trim().is_empty());
    lines.join("\n")
}

/// Remembers the last action that changed nothing visible.
#[derive(Debug, Default)]
pub(crate) struct Progress {
    /// The action, and the window as it looked afterwards.
    stuck: Option<(Action, Observation)>,
}

impl Progress {
    /// Whether `action` is the one that just changed nothing, on a window that
    /// still looks the same. Sending it again would only burn a turn.
    pub(crate) fn repeats(&self, action: &Action, now: &Observation) -> bool {
        self.stuck
            .as_ref()
            .is_some_and(|(last, after)| last == action && looks_the_same(after, now))
    }

    /// Records what `action`, sent on `before`, came to. A refused action did
    /// nothing, so it changes nothing here. Actions that aren't meant to change
    /// the window (observe, read, wait) are never held against the model.
    pub(crate) fn record(&mut self, action: &Action, before: &Observation, outcome: &ActionResult) {
        if outcome.refused.is_some() {
            return;
        }
        let unchanged = hands::needs_verify(action) && looks_the_same(before, &outcome.observation);
        self.stuck = unchanged.then(|| (action.clone(), outcome.observation.clone()));
    }
}

/// Everything the model can see is the same: the window, its size, the
/// screenshot and the outline. The token always differs, so it doesn't count.
fn looks_the_same(a: &Observation, b: &Observation) -> bool {
    a.window == b.window && a.size == b.size && a.image == b.image && a.outline == b.outline
}

/// The loop's own refusal of a repeated action: nothing was sent, and here is
/// a fresh look at the window.
pub(crate) fn no_visible_progress(observation: Observation) -> ActionResult {
    ActionResult {
        ok: false,
        refused: Some(Refusal::NoVisibleProgress),
        observation,
        verify_required: false,
    }
}

/// Added to the report of a `no_visible_progress` refusal, so the model knows
/// what to do differently.
pub(crate) const TRY_SOMETHING_ELSE: &str = "That exact action changed nothing visible last \
time, and the window still looks the same, so it wasn't sent again. Try something else.";

#[cfg(test)]
mod tests {
    use super::*;
    use proto::{Element, ElementOp, Rect, Size, WindowInfo};
    use serde_json::json;

    fn observation(token: &str, switch: &str) -> Observation {
        Observation {
            token: token.into(),
            window: WindowInfo {
                id: "w1".into(),
                title: "Settings".into(),
                app: "settings".into(),
            },
            size: Size {
                width: 1280,
                height: 800,
            },
            image: Some("data:image/png;base64,AAAA".into()),
            outline: Some(vec![Element {
                id: "e12".into(),
                role: "switch".into(),
                name: "Dark mode".into(),
                value: Some(switch.into()),
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: 70,
                    height: 36,
                },
                actions: vec!["toggle".into()],
            }]),
        }
    }

    fn press(element: &str) -> Action {
        Action::Act {
            element: element.into(),
            op: ElementOp::Click,
            text: None,
        }
    }

    fn result(observation: Observation) -> ActionResult {
        ActionResult {
            ok: true,
            refused: None,
            observation,
            verify_required: true,
        }
    }

    #[test]
    fn an_action_that_changed_nothing_is_held_back() {
        let mut progress = Progress::default();
        let before = observation("t1", "off");
        progress.record(&press("e12"), &before, &result(observation("t2", "off")));
        let now = observation("t3", "off");
        assert!(progress.repeats(&press("e12"), &now));
        // Another action is fine.
        assert!(!progress.repeats(&press("e3"), &now));
    }

    #[test]
    fn an_action_that_changed_something_can_be_sent_again() {
        let mut progress = Progress::default();
        let before = observation("t1", "off");
        progress.record(&press("e12"), &before, &result(observation("t2", "on")));
        assert!(!progress.repeats(&press("e12"), &observation("t3", "on")));
    }

    #[test]
    fn a_window_that_changed_since_lets_it_through() {
        let mut progress = Progress::default();
        let before = observation("t1", "off");
        progress.record(&press("e12"), &before, &result(observation("t2", "off")));
        // Something else (the app itself, a slow load) changed the window.
        assert!(!progress.repeats(&press("e12"), &observation("t3", "on")));
    }

    #[test]
    fn looking_and_waiting_are_never_held_back() {
        let mut progress = Progress::default();
        let before = observation("t1", "off");
        for passive in [Action::Observe, Action::Read, Action::Wait { ms: 500 }] {
            progress.record(&passive, &before, &result(observation("t2", "off")));
            assert!(!progress.repeats(&passive, &observation("t3", "off")));
        }
    }

    #[test]
    fn a_refused_action_did_nothing_so_it_counts_for_nothing() {
        let mut progress = Progress::default();
        let before = observation("t1", "off");
        progress.record(&press("e12"), &before, &result(observation("t2", "off")));
        let stale = ActionResult {
            ok: false,
            refused: Some(Refusal::StaleToken),
            ..result(observation("t3", "off"))
        };
        progress.record(&press("e3"), &observation("t2", "off"), &stale);
        // Still stuck on e12: the refused e3 changed nothing either way.
        assert!(progress.repeats(&press("e12"), &observation("t4", "off")));
    }

    #[test]
    fn older_observations_read_as_one_line() {
        let line = trail_line(&observation("t1", "off"), true);
        assert_eq!(
            line,
            "(Older observation, shrunk to one line: window \"Settings\" (settings), \
             1280x800 px, 1 control, screenshot dropped.)"
        );
        assert!(!line.contains("t1"), "the token stays with the loop");
        assert!(!trail_line(&observation("t1", "off"), false).contains("screenshot"));
    }

    #[test]
    fn the_trail_shrinks_the_older_observation_only() {
        let full = |head: &str| {
            vec![
                ContentBlock::text(head),
                ContentBlock::text("Window ... Outline: [...]"),
                ContentBlock::Image {
                    media_type: "image/png".into(),
                    base64: "AAAA".into(),
                },
            ]
        };
        let mut messages = vec![
            Message::user(full("Task: x")),
            Message::assistant(vec![]),
            Message::user(vec![ContentBlock::ToolResult {
                id: "c1".into(),
                content: full("Done. Here is the window now."),
                is_error: false,
            }]),
        ];
        let mut trail = Trail::default();
        let first = observation("t1", "off");
        trail.push(&mut messages, Slot::Message(0), &first, true);
        assert_eq!(messages[0].content.len(), 3, "the newest stays in full");

        let second = observation("t2", "on");
        let slot = Slot::Result {
            message: 2,
            block: 0,
        };
        trail.push(&mut messages, slot, &second, true);
        assert_eq!(
            messages[0].content,
            [
                ContentBlock::text("Task: x"),
                ContentBlock::text(trail_line(&first, true)),
            ]
        );
        let ContentBlock::ToolResult { content, .. } = &messages[2].content[0] else {
            panic!("not a tool result");
        };
        assert_eq!(content.len(), 3, "the newest stays in full");
    }

    #[test]
    fn step_notes_keep_what_the_model_wrote_down() {
        let content = vec![
            ContentBlock::text("Opening Appearance."),
            ContentBlock::ToolCall {
                id: "c1".into(),
                name: "note".into(),
                args: json!({"text": "Dark mode lives under Appearance."}),
            },
            ContentBlock::ToolCall {
                id: "c2".into(),
                name: "computer".into(),
                args: json!({"do": "act", "element": "e3", "op": "click"}),
            },
        ];
        assert_eq!(
            step_note(&content),
            "Opening Appearance.\nDark mode lives under Appearance."
        );
        assert_eq!(step_note(&[]), "");
    }
}
