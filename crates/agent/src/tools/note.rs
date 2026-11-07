use proto::ToolSpec;
use serde_json::{Value, json};

use crate::{Call, Tool};

/// The model writes down what it learned and what's left. Older observations
/// shrink to one line each (see `memory.rs`), so this is how it keeps what it
/// needs from them. Not an action: it can go with one `computer` call.
pub struct Note;

impl Tool for Note {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "note".into(),
            description: "Write down what you've learned and what's left to do. Older \
                          observations shrink to one line each, so note anything from them \
                          you'll need later. Not an action: it can go with one `computer` call \
                          in the same turn."
                .into(),
            schema: json!({
                "type": "object",
                "properties": {"text": {"type": "string", "description": "The note, in a sentence or two."}},
                "required": ["text"]
            }),
        }
    }

    fn parse(&self, args: &Value) -> anyhow::Result<Call> {
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| anyhow::anyhow!("`note` needs a non-empty `text` string"))?;
        Ok(Call::Note {
            text: text.to_owned(),
        })
    }
}
