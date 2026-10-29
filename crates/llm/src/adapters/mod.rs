// List-only file: one `mod` line and one `ALL` line per adapter, sorted.
// .gitattributes merges it with `merge=union`, so two PRs that each add an
// adapter don't conflict here. Keep anything else out of this file.

mod anthropic;
mod antigravity;
mod claude_code;
mod codex;
mod gemini;
mod openai;
mod openai_compat;

pub const ALL: &[crate::Adapter] = &[
    anthropic::ADAPTER,
    antigravity::ADAPTER,
    claude_code::ADAPTER,
    codex::ADAPTER,
    gemini::ADAPTER,
    openai::ADAPTER,
    openai_compat::ADAPTER,
];
