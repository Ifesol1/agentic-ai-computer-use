//! `device`: runs on a member's PC. It will dial the hub over a WebSocket (so no
//! port forwarding), take one action at a time, and enforce the safety rules
//! (tokens, budgets, the person always wins) on the PC itself.

fn main() {
    eprintln!("device {}: not built yet", env!("CARGO_PKG_VERSION"));
    std::process::exit(2);
}
