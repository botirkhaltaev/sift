use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use clap::Parser;
use sift_grep::index::daemon::{DaemonOrchestrator, ServeConfig};

#[derive(Parser)]
#[command(version, about = "Background index refresher for sift")]
struct DaemonArgs {
    #[arg(long, default_value = ".sift")]
    sift_dir: PathBuf,

    #[arg(long)]
    init_root: Option<PathBuf>,

    /// Internal: startup handshake file created after the watcher is active.
    #[arg(long, hide = true)]
    ready_file: Option<PathBuf>,

    /// Seconds of inactivity before the daemon exits.  Defaults to 120
    /// (2 minutes).  Set to 0 to disable the idle timeout.
    #[arg(long, default_value_t = 120)]
    idle_timeout_secs: u64,
}

fn main() {
    let args = DaemonArgs::parse();
    let idle_timeout = if args.idle_timeout_secs == 0 {
        Duration::from_hours(876_000)
    } else {
        Duration::from_secs(args.idle_timeout_secs)
    };
    let daemon = DaemonOrchestrator::new(args.sift_dir, args.init_root);
    let shutdown = AtomicBool::new(false);
    if let Err(e) = daemon.serve(
        ServeConfig {
            ready: args.ready_file,
            idle_timeout,
        },
        &shutdown,
    ) {
        eprintln!("sift-daemon: {e}");
        std::process::exit(1);
    }
}
