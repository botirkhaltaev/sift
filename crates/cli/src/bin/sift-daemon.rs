use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Background index refresher for sift")]
struct DaemonArgs {
    #[arg(long, default_value = ".sift")]
    sift_dir: PathBuf,

    #[arg(long)]
    init_root: Option<PathBuf>,

    /// Build or update once, then exit instead of watching for changes.
    #[arg(long)]
    once: bool,

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
        // Effectively infinite — ~100 years; small enough to avoid
        // Instant overflow on all platforms.
        Duration::from_hours(876_000)
    } else {
        Duration::from_secs(args.idle_timeout_secs)
    };
    let config = sift_grep::daemon::DaemonRunConfig {
        sift_dir: args.sift_dir,
        init_root: args.init_root,
        ready_file: args.ready_file,
        idle_timeout,
    };
    let runner = sift_grep::daemon::DaemonRunner::new(config);
    let result = if args.once {
        runner.run_once()
    } else {
        runner.run()
    };
    if let Err(e) = result {
        eprintln!("sift-daemon: {e}");
        std::process::exit(1);
    }
}
