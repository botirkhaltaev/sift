//! CLI Criterion benchmarks — subprocess-based, covers walk, indexed, filter,
//! and output modes.  Each iteration runs the real `sift` binary so startup
//! overhead, parsing, search, and printing are all captured.
//!
//! Run:   cargo bench -p sift-cli --bench cli
//! Profile: cargo bench -p sift-cli --bench cli -- --profile-time 30

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sift"))
}

fn sift_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .sample_size(100)
        .configure_from_args()
}

fn run_sift(args: &[&str], cwd: &Path) {
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

// ── Corpus helpers ──────────────────────────────────────────────────────────

fn make_small_corpus(root: &Path, n: usize) {
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

fn make_filter_corpus(root: &Path) {
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

fn build_index(corpus: &Path, sift_dir: &Path) {
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

// ─── type-list (pure CLI, no filesystem walk) ───────────────────────────────

fn bench_type_list(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let mut g = c.benchmark_group("type_list");
    g.bench_function("list_types", |b| {
        b.iter(|| run_sift(&["--type-list"], tmp.path()));
    });
    g.finish();
}

// ─── Walk mode (no index) ──────────────────────────────────────────────────

fn bench_walk_search(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    make_small_corpus(&corpus, 20);
    let no_index = tmp.path().join(".sift-absent");

    let mut g = c.benchmark_group("walk_search");

    g.bench_function("literal_20_files", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "beta"],
                &corpus,
            );
        });
    });

    g.bench_function("literal_json", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "--json", "beta"],
                &corpus,
            );
        });
    });

    g.bench_function("count", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "-c", "beta"],
                &corpus,
            );
        });
    });

    g.bench_function("files_with_matches", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "-l", "beta"],
                &corpus,
            );
        });
    });

    g.bench_function("no_literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), r"\w{10}"],
                &corpus,
            );
        });
    });

    g.finish();
}

// ─── Indexed mode ──────────────────────────────────────────────────────────

fn bench_indexed_search(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    make_small_corpus(&corpus, 20);
    let sift_dir = tmp.path().join(".sift");
    build_index(&corpus, &sift_dir);

    let mut g = c.benchmark_group("indexed_search");

    g.bench_function("literal_20_files", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &sift_dir.to_string_lossy(), "beta"],
                &corpus,
            );
        });
    });

    g.bench_function("literal_json", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &sift_dir.to_string_lossy(), "--json", "beta"],
                &corpus,
            );
        });
    });

    g.bench_function("count", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &sift_dir.to_string_lossy(), "-c", "beta"],
                &corpus,
            );
        });
    });

    g.bench_function("no_literal", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &sift_dir.to_string_lossy(), r"\w{10}"],
                &corpus,
            );
        });
    });

    g.finish();
}

// ─── Filter / glob modes (walk) ────────────────────────────────────────────

fn bench_filter(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    make_filter_corpus(&corpus);
    let no_index = tmp.path().join(".sift-absent");

    let mut g = c.benchmark_group("filter");

    g.bench_function("default", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "beta"],
                &corpus,
            );
        });
    });

    g.bench_function("hidden_included", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &no_index.to_string_lossy(),
                    "--hidden",
                    "beta",
                ],
                &corpus,
            );
        });
    });

    g.bench_function("glob_rust", |b| {
        b.iter(|| {
            run_sift(
                &[
                    "--sift-dir",
                    &no_index.to_string_lossy(),
                    "-g",
                    "*.rs",
                    "beta",
                ],
                &corpus,
            );
        });
    });

    g.finish();
}

// ─── --files mode (file traversal only) ────────────────────────────────────

fn bench_files_mode(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    make_small_corpus(&corpus, 20);
    let no_index = tmp.path().join(".sift-absent");

    let mut g = c.benchmark_group("files_mode");

    g.bench_function("list_all", |b| {
        b.iter(|| {
            run_sift(
                &["--sift-dir", &no_index.to_string_lossy(), "--files"],
                &corpus,
            );
        });
    });

    g.finish();
}

criterion_group!(
    name = cli;
    config = sift_criterion();
    targets = bench_type_list, bench_walk_search, bench_indexed_search, bench_filter, bench_files_mode
);
criterion_main!(cli);
