use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use clap::Parser;
use criterion::Criterion;

use sift_cli::cli::Cli;

pub fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sift"))
}

pub fn sift_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .sample_size(100)
        .configure_from_args()
}

pub fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

pub fn run_sift(args: &[&str], cwd: &Path) {
    let out = Command::new(exe())
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sift exited {}: {}",
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr),
    );
}

pub fn parse_cli(argv: &[&str]) -> Cli {
    let full: Vec<&str> = std::iter::once("sift")
        .chain(argv.iter().copied())
        .collect();
    Cli::try_parse_from(full).unwrap()
}

pub fn make_small_corpus(root: &Path, n: usize) {
    for i in 0..n {
        let dir = root.join(format!("pkg{i:04}"));
        std::fs::create_dir_all(&dir).unwrap();
        let content: String = (0..30)
            .map(|j| {
                if j % 7 == 3 {
                    format!("// {i}:{j} fn beta_{j}(){{}}\n")
                } else if j % 11 == 5 {
                    format!("// {i}:{j} let loooooooooong_var_{i}_{j} = {j};\n")
                } else {
                    format!("// {i}:{j} fn alpha_{j}(){{}}\n")
                }
            })
            .collect();
        std::fs::write(dir.join("lib.rs"), content).unwrap();
    }
}

pub fn make_filter_corpus(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("src/.hidden")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn hello() {}\nfn beta_helper() {}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/.hidden/secret.rs"), "fn beta_here() {}\n").unwrap();
    std::fs::write(root.join("tests/test1.rs"), "fn test_beta() {}\n").unwrap();
    std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
}

pub fn build_index(corpus: &Path, sift_dir: &Path) {
    let out = Command::new(exe())
        .arg("--sift-dir")
        .arg(sift_dir)
        .arg("build")
        .arg(corpus)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "index build failed: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}
