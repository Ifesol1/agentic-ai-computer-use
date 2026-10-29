use proto::{Action, ToolSpec};
use serde_json::{Value, json};

use crate::{Call, Tool};

/// The generic computer tool: its arguments are exactly a `proto::Action`, so the
/// tool surface is the one frozen in `proto`. Native tools (Claude's, Gemini's)
/// are mapped onto the same actions by their adapters.
pub struct Computer;

impl Tool for Computer {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "computer".into(),
            description: "Do one thing in the window. Prefer `act` on an outline element \
                          (e.g. e12) over clicking by position. Every call returns a fresh \
                          observation; check it before you call `done`."
                .into(),
            schema: json!({
                "type": "object",
                "properties": {
                    "do": {"enum": ["observe", "act", "click", "move", "scroll", "drag", "key", "type_text", "read", "wait"]},
                    "element": {"type": "string", "description": "act: an outline reference such as e12"},
                    "op": {"enum": ["click", "double_click", "right_click", "type", "scroll"]},
                    "text": {"type": "string"},
                    "x": {"type": "integer"}, "y": {"type": "integer"},
                    "dx": {"type": "integer"}, "dy": {"type": "integer"},
                    "button": {"enum": ["left", "right", "middle"]},
                    "count": {"type": "integer", "minimum": 1, "maximum": 2},
                    "from": {"type": "object"}, "to": {"type": "object"},
                    "ms": {"type": "integer"},
                    "keys": {"type": "string", "description": "a key or chord, e.g. ctrl+s"}
                },
                "required": ["do"]
            }),
        }
    }

    fn parse(&self, args: &Value) -> anyhow::Result<Call> {
        let action: Action = serde_json::from_value(args.clone())
            .map_err(|error| anyhow::anyhow!("not a valid `computer` call: {error}"))?;
        Ok(Call::Act(action))
    }
}
