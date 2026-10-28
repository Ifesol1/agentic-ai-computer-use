//! The scripted "turn on dark mode" run that `harness run --mock` plays against
//! `hands::fake::FakeSettings`: the real loop, no keys, no desktop.

use llm::{ContentBlock, MockProvider};
use serde_json::json;

pub fn script() -> Vec<Vec<ContentBlock>> {
    let press = |element: &str| json!({"do": "act", "element": element, "op": "click"});
    vec![
        MockProvider::call(
            "Appearance is in the sidebar (e3). Opening it.",
            "call-1",
            "computer",
            press("e3"),
        ),
        MockProvider::call(
            "The Dark mode switch (e12) is off. Turning it on.",
            "call-2",
            "computer",
            press("e12"),
        ),
        MockProvider::call(
            "The newest observation shows the switch on.",
            "call-3",
            "done",
            json!({"reply": "Dark mode is on: the switch shows on."}),
        ),
    ]
}
