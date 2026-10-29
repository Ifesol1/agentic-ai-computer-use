use proto::ToolSpec;
use serde_json::{Value, json};

use crate::{Call, Tool};

/// Ends the run with the final answer. Refused in the same turn as an action:
/// the action's result has to be seen first (no unverified success).
pub struct Done;

impl Tool for Done {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "done".into(),
            description: "Finish the task. Call it only after an observation shows the result, \
                          and say what you saw."
                .into(),
            schema: json!({
                "type": "object",
                "properties": {"reply": {"type": "string", "description": "The final answer for the person."}},
                "required": ["reply"]
            }),
        }
    }

    fn parse(&self, args: &Value) -> anyhow::Result<Call> {
        let reply = args
            .get("reply")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("`done` needs a `reply` string"))?;
        Ok(Call::Done {
            reply: reply.to_owned(),
        })
    }
}
