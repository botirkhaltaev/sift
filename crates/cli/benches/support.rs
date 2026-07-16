use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use clap::Parser;
use criterion::Criterion;

use sift_grep::cli::Cli;

/// Matches `sift-core` bench search scale (see core `CorpusScale::SEARCH`).
#[derive(Debug, Clone, Copy)]
pub struct CorpusScale {
    pub files: usize,
    pub lines_per_file: usize,
}

impl CorpusScale {
    pub const SEARCH: Self = Self {
        files: 32_000,
        lines_per_file: 160,
    };
    pub const STRESS: Self = Self {
        files: 64_000,
        lines_per_file: 200,
    };
    pub const CI: Self = Self {
        files: 8_000,
        lines_per_file: 100,
    };
}

#[must_use]
pub fn search_scale() -> CorpusScale {
    match env::var("SIFT_BENCH_SCALE")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "ci" | "small" => CorpusScale::CI,
        "stress" | "large" => CorpusScale::STRESS,
        _ => CorpusScale::SEARCH,
    }
}

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

/// Monorepo-scale synthetic corpus (same shape as sift-core search fixtures).
pub fn materialize_search_corpus(root: &Path, scale: CorpusScale) {
    let fanout = 512usize;
    for i in 0..scale.files {
        let c = i % fanout;
        let path = root
            .join("crates")
            .join(format!("c{c:04}"))
            .join("src")
            .join(format!("module_{i}.rs"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let f = std::fs::File::create(&path).unwrap();
        let mut f = std::io::BufWriter::new(f);
        use std::io::Write;
        for line in 0..scale.lines_per_file {
            let mid = if line % 47 == 3 {
                " beta "
            } else if line % 91 == 7 {
                " RESUME "
            } else if line % 31 == 11 {
                " ERR_SYS "
            } else {
                " xval "
            };
            writeln!(
                f,
                "// {i}:{line} fn sym_{line}(){mid} struct Row{{ id: u32 }} // padding_{i}_{line}"
            )
            .unwrap();
        }
    }
}

fn cli_fixture_root(scale: CorpusScale) -> PathBuf {
    let target = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    target.join("sift-bench-fixtures").join(format!(
        "cli-search-f{}-l{}",
        scale.files, scale.lines_per_file
    ))
}

/// Cached corpus + `.sift` index for large CLI e2e benches.
pub fn large_indexed_fixture() -> (PathBuf, PathBuf) {
    static PATHS: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PATHS
        .get_or_init(|| {
            let scale = search_scale();
            let root = cli_fixture_root(scale);
            let ready = root.join("READY");
            let corpus = root.join("corpus");
            let sift_dir = root.join(".sift");
            if !(ready.is_file() && corpus.is_dir() && sift_dir.is_dir()) {
                let _ = std::fs::remove_dir_all(&root);
                std::fs::create_dir_all(&corpus).unwrap();
                eprintln!(
                    "sift-bench(cli): materializing {} files × {} lines under {}",
                    scale.files,
                    scale.lines_per_file,
                    root.display()
                );
                materialize_search_corpus(&corpus, scale);
                build_index(&corpus, &sift_dir);
                std::fs::write(&ready, b"ok\n").unwrap();
            }
            (corpus, sift_dir)
        })
        .clone()
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
        .args(["index", "build", "--wait"])
        .arg(corpus)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "index build failed: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}
