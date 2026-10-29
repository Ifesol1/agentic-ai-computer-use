// List-only file: one `mod` line and one `ALL` line per tool, sorted.
// .gitattributes merges it with `merge=union`, so two PRs that each add a tool
// don't conflict here. The `Tool` trait lives in src/tool.rs.

mod computer;
mod done;

// One entry per line, so union merges keep both sides.
#[rustfmt::skip]
pub const ALL: &[&dyn crate::Tool] = &[
    &computer::Computer,
    &done::Done,
];
