//! Corpus fixtures and `SIFT_PROFILE_*` environment for `sift-profile`.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sift_core::{Index, IndexBuilder};

use crate::metrics::print_profile;

#[derive(Clone, Debug)]
pub enum CorpusKind {
    Parity,
    Filter,
    Large {
        files: usize,
        lines_per_file: usize,
        dir_fanout: usize,
    },
}

pub fn corpus_kind_from_env() -> CorpusKind {
    let large = std::env::var("SIFT_PROFILE_LARGE")
        .is_ok_and(|s| s == "1" || s.eq_ignore_ascii_case("true"));
    let files = std::env::var("SIFT_PROFILE_CORPUS_FILES")
        .ok()
        .and_then(|s| s.parse().ok());

    if large && files.is_none() {
        return CorpusKind::Large {
            files: 8_000,
            lines_per_file: std::env::var("SIFT_PROFILE_CORPUS_LINES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
            dir_fanout: std::env::var("SIFT_PROFILE_CORPUS_DIRS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(256),
        };
    }

    match files {
        None | Some(0) => {
            if std::env::var("SIFT_PROFILE_FILTER_CORPUS").is_ok() {
                CorpusKind::Filter
            } else {
                CorpusKind::Parity
            }
        }
        Some(n) => CorpusKind::Large {
            files: n,
            lines_per_file: std::env::var("SIFT_PROFILE_CORPUS_LINES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(120),
            dir_fanout: std::env::var("SIFT_PROFILE_CORPUS_DIRS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(256),
        },
    }
}

/// Parity corpus: a/x.txt ("alpha beta"), b/y.txt ("gamma delta").
pub fn make_parity_corpus(root: &Path) {
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a/x.txt"), "alpha beta\n").unwrap();
    fs::write(root.join("b/y.txt"), "gamma delta\n").unwrap();
}

/// Filter-testing corpus with mixed file types, hidden files, scoped subdirs,
/// and ignore markers.
pub fn make_filter_corpus(root: &Path) {
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("a/.secret")).unwrap();
    fs::create_dir_all(root.join("subdir")).unwrap();
    fs::create_dir_all(root.join("skip")).unwrap();
    fs::create_dir_all(root.join("also_skip")).unwrap();

    fs::write(root.join("a/x.txt"), "alpha beta gamma\n").unwrap();
    fs::write(root.join("a/.hidden.txt"), "beta in hidden file\n").unwrap();
    fs::write(root.join("a/data.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("a/.secret/log"), "beta in hidden dir\n").unwrap();
    fs::write(root.join("subdir/a.txt"), "beta in subdir\n").unwrap();
    fs::write(root.join("subdir/b.log"), "no match here\n").unwrap();
    fs::write(root.join("root.txt"), "beta at root level\n").unwrap();
    fs::write(root.join("skip/ignored.txt"), "beta gitignored\n").unwrap();
    fs::write(root.join("also_skip/omit.txt"), "beta in .ignore\n").unwrap();
    fs::write(root.join("keep.txt"), "beta outside ignore rules\n").unwrap();

    fs::write(root.join(".gitignore"), "skip/\n").unwrap();
    fs::write(root.join(".ignore"), "also_skip/\n").unwrap();
}

/// Monorepo-shaped tree: `crates/cNNNN/src/module_M.rs` with many lines of pseudo-Rust.
pub fn materialize_large_corpus(
    root: &Path,
    files: usize,
    lines_per_file: usize,
    dir_fanout: usize,
) {
    let fanout = dir_fanout.max(1);
    for i in 0..files {
        let c = i % fanout;
        let path = root
            .join("crates")
            .join(format!("c{c:04}"))
            .join("src")
            .join(format!("module_{i}.rs"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = fs::File::create(&path).unwrap();
        for line in 0..lines_per_file {
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
                "// {i}:{line} fn sym_{line}(){mid} struct Row{{ id: u32 }}"
            )
            .unwrap();
        }
    }
}

pub fn make_many_files_corpus(root: &Path, n: usize) {
    for i in 0..n {
        let dir = root.join(format!("d{}", i % 10));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("f{i}.txt")),
            format!("line one line two content {i}\n"),
        )
        .unwrap();
    }
}

fn materialize_search_corpus(root: &Path, kind: &CorpusKind) {
    match kind {
        CorpusKind::Parity => make_parity_corpus(root),
        CorpusKind::Filter => make_filter_corpus(root),
        CorpusKind::Large {
            files,
            lines_per_file,
            dir_fanout,
        } => materialize_large_corpus(root, *files, *lines_per_file, *dir_fanout),
    }
}

pub fn materialize_build_corpus(root: &Path, kind: &CorpusKind) {
    match kind {
        CorpusKind::Parity | CorpusKind::Filter => make_many_files_corpus(root, 32),
        CorpusKind::Large {
            files,
            lines_per_file,
            dir_fanout,
        } => materialize_large_corpus(root, *files, *lines_per_file, *dir_fanout),
    }
}

fn external_corpus_paths() -> Option<(PathBuf, PathBuf)> {
    let corpus = std::env::var_os("SIFT_PROFILE_CORPUS").map(PathBuf::from)?;
    let index = std::env::var_os("SIFT_PROFILE_INDEX").map_or_else(
        || PathBuf::from(format!("{}.sift", corpus.display())),
        PathBuf::from,
    );
    Some((corpus, index))
}

pub fn open_corpus_index(kind: &CorpusKind) -> (tempfile::TempDir, Index) {
    if let Some((corpus, index_dir)) = external_corpus_paths() {
        let t_open = Instant::now();
        let index = Index::open(&index_dir).unwrap();
        let open_ms = t_open.elapsed().as_secs_f64() * 1e3;
        print_profile("corpus_kind", "external");
        print_profile("corpus_root", &corpus.display().to_string());
        print_profile("index_root", &index_dir.display().to_string());
        print_profile("phase_open_index_ms", &format!("{open_ms:.3}"));
        return (tempfile::tempdir().unwrap(), index);
    }

    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");

    let t_mat = Instant::now();
    materialize_search_corpus(&corpus, kind);
    let mat_ms = t_mat.elapsed().as_secs_f64() * 1e3;
    print_profile("phase_materialize_corpus_ms", &format!("{mat_ms:.3}"));

    match kind {
        CorpusKind::Parity => {
            print_profile("corpus_kind", "parity");
            print_profile("corpus_files", "2");
        }
        CorpusKind::Filter => {
            print_profile("corpus_kind", "filter");
            print_profile("corpus_files", "12");
        }
        CorpusKind::Large {
            files,
            lines_per_file,
            dir_fanout,
        } => {
            print_profile("corpus_kind", "large");
            print_profile("corpus_files", &files.to_string());
            print_profile("corpus_lines_per_file", &lines_per_file.to_string());
            print_profile("corpus_dir_fanout", &dir_fanout.to_string());
        }
    }

    let idx = tmp.path().join(".sift");
    let t0 = Instant::now();
    let _ = IndexBuilder::new(&corpus).with_dir(&idx).build().unwrap();
    let build_ms = t0.elapsed().as_secs_f64() * 1e3;
    let t1 = Instant::now();
    let index = Index::open(&idx).unwrap();
    let open_ms = t1.elapsed().as_secs_f64() * 1e3;
    print_profile("phase_build_index_ms", &format!("{build_ms:.3}"));
    print_profile("phase_open_index_ms", &format!("{open_ms:.3}"));
    (tmp, index)
}
