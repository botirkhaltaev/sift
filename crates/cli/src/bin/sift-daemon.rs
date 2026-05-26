use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Background index refresher for sift")]
struct DaemonArgs {
    #[arg(long, default_value = ".sift")]
    sift_dir: PathBuf,

    #[arg(long)]
    init_root: Option<PathBuf>,
}

fn main() {
    let args = DaemonArgs::parse();
    let config = sift_cli::daemon::DaemonConfig {
        sift_dir: args.sift_dir,
        init_root: args.init_root,
    };
    if let Err(e) = config.run() {
        eprintln!("sift-daemon: {e}");
        std::process::exit(1);
    }
}
