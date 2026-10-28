use proto::{Action, ToolSpec};
use serde_json::Value;

/// A tool the model can call. Tools only parse; the loop does the work, so
/// tokens, verify and budgets apply the same to every tool.
pub trait Tool: Sync {
    fn spec(&self) -> ToolSpec;
    fn parse(&self, args: &Value) -> anyhow::Result<Call>;
}

/// What a tool call asks the loop to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Act(Action),
    Done { reply: String },
}

/// Every tool, in the order the model sees them.
pub fn all() -> &'static [&'static dyn Tool] {
    crate::tools::ALL
}

pub fn find(name: &str) -> Option<&'static dyn Tool> {
    all().iter().copied().find(|tool| tool.spec().name == name)
}
