//! `cargo dev`: builds the server and the app, starts `harness serve --mock`, then
//! opens the app. Closing the app stops the server.
//!
//! If something already answers on 127.0.0.1:7878 (say a backend dev's own
//! `cargo run -p agent -- serve --mock`), the app uses that instead, so frontend
//! and backend can each restart their side without stopping the other.

use std::{
    env,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode},
    thread,
    time::{Duration, Instant},
};

const ADDR: &str = "127.0.0.1:7878";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dev") => dev(),
        _ => Err("usage: cargo dev".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cargo dev: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dev() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("xtask lives in crates/xtask");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let built = Command::new(cargo)
        .args(["build", "-p", "agent", "-p", "app"])
        .current_dir(root)
        .status()
        .map_err(|error| format!("can't run cargo: {error}"))?;
    if !built.success() {
        return Err("the build failed".into());
    }

    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let exe = |name: &str| {
        target
            .join("debug")
            .join(format!("{name}{}", env::consts::EXE_SUFFIX))
    };

    let addr: SocketAddr = ADDR.parse().unwrap();
    let mut server = None;
    if answers(addr) {
        println!("cargo dev: using the server already on {ADDR}");
    } else {
        let child = Command::new(exe("harness"))
            .args(["serve", "--mock", "--addr", ADDR])
            .spawn()
            .map_err(|error| format!("can't start harness: {error}"))?;
        server = Some(Stopper(child));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !answers(addr) {
            if Instant::now() > deadline {
                return Err(format!("the mock server didn't come up on {ADDR}"));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    let app = Command::new(exe("app"))
        .env("HARNESS_URL", format!("http://{ADDR}"))
        .status()
        .map_err(|error| format!("can't start the app: {error}"))?;
    drop(server);
    if app.success() {
        Ok(())
    } else {
        Err(format!("the app exited with {app}"))
    }
}

fn answers(addr: SocketAddr) -> bool {
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

/// Stops the server when `cargo dev` ends, however it ends.
struct Stopper(Child);

impl Drop for Stopper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
