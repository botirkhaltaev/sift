use std::path::PathBuf;

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
}

fn main() {
    let args = DaemonArgs::parse();
    let config = sift_grep::daemon::DaemonRunConfig {
        sift_dir: args.sift_dir,
        init_root: args.init_root,
        ready_file: args.ready_file,
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
