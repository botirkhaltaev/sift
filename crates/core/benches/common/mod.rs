//! Shared fixtures and helpers for sift-core benchmarks.
//!
//! All fixtures are deterministic and use temporary directories that are
//! automatically cleaned up. Search/open/candidate benches build fixtures
//! outside `b.iter`; build benches materialize inside `b.iter`.

#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sift_core::{
    ColorChoice, CompiledSearch, FilenameMode, GlobConfig, HiddenMode, IgnoreConfig, IgnoreSources,
    LineStyleFlags, OutputEmission, PathDisplay, SearchFilter, SearchFilterConfig, SearchLineStyle,
    SearchMode, SearchOptions, SearchOutput, SearchOutputFormat, SearchRecordStyle,
    SearchSeparators, TrigramIndex, TrigramIndexBuilder, VisibilityConfig,
};

// ─── Corpus materializers ────────────────────────────────────────────────────

pub fn make_parity_corpus(root: &Path) {
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a/x.txt"), "alpha beta\n").unwrap();
    fs::write(root.join("b/y.txt"), "gamma delta\n").unwrap();
}

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

pub fn make_single_file_corpus(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("single.rs"),
        "fn main() {\n    let x = 42;\n    println!(\"beta: {}\", x);\n}\n",
    )
    .unwrap();
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
        let f = fs::File::create(&path).unwrap();
        let mut f = std::io::BufWriter::new(f);
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

pub fn materialize_monorepo_corpus(
    root: &Path,
    files: usize,
    lines_per_file: usize,
    dir_fanout: usize,
) {
    materialize_large_corpus(root, files, lines_per_file, dir_fanout);
}

// ─── Index helpers ───────────────────────────────────────────────────────────

pub fn build_index(corpus: &Path, idx_dir: &Path) -> TrigramIndex {
    TrigramIndexBuilder::new(corpus)
        .with_dir(idx_dir)
        .build()
        .unwrap()
}

pub fn open_index(idx_dir: &Path) -> TrigramIndex {
    TrigramIndex::open(idx_dir).unwrap()
}

pub fn open_parity_index() -> (tempfile::TempDir, TrigramIndex) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    make_parity_corpus(&corpus);
    let idx = tmp.path().join(".sift");
    build_index(&corpus, &idx);
    let index = open_index(&idx);
    (tmp, index)
}

pub fn open_filter_index() -> (tempfile::TempDir, TrigramIndex) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    make_filter_corpus(&corpus);
    let idx = tmp.path().join(".sift");
    build_index(&corpus, &idx);
    let index = open_index(&idx);
    (tmp, index)
}

pub fn open_large_index() -> (tempfile::TempDir, TrigramIndex) {
    let tmp = tempfile::tempdir().unwrap();
    let corpus = tmp.path().join("corpus");
    materialize_large_corpus(&corpus, 8_000, 100, 256);
    let idx = tmp.path().join(".sift");
    build_index(&corpus, &idx);
    let index = open_index(&idx);
    (tmp, index)
}

// ─── Filter configs ─────────────────────────────────────────────────────────

pub fn default_filter() -> SearchFilterConfig {
    SearchFilterConfig {
        visibility: VisibilityConfig {
            hidden: HiddenMode::Respect,
            ignore: IgnoreConfig {
                sources: IgnoreSources::DOT | IgnoreSources::VCS | IgnoreSources::EXCLUDE,
                custom_files: Vec::new(),
                require_git: true,
            },
        },
        ..SearchFilterConfig::default()
    }
}

pub fn glob_include_filter() -> SearchFilterConfig {
    SearchFilterConfig {
        glob: GlobConfig {
            patterns: vec!["**/*.txt".to_string()],
            case_insensitive: false,
        },
        ..default_filter()
    }
}

pub fn glob_exclude_filter() -> SearchFilterConfig {
    SearchFilterConfig {
        glob: GlobConfig {
            patterns: vec!["!**/*.txt".to_string()],
            case_insensitive: false,
        },
        ..default_filter()
    }
}

pub fn glob_casei_filter() -> SearchFilterConfig {
    SearchFilterConfig {
        glob: GlobConfig {
            patterns: vec!["**/*.TXT".to_string()],
            case_insensitive: true,
        },
        ..default_filter()
    }
}

pub fn hidden_include_filter() -> SearchFilterConfig {
    SearchFilterConfig {
        visibility: VisibilityConfig {
            hidden: HiddenMode::Include,
            ..default_filter().visibility
        },
        ..default_filter()
    }
}

pub fn ignore_custom_filter() -> SearchFilterConfig {
    SearchFilterConfig {
        visibility: VisibilityConfig {
            hidden: HiddenMode::Respect,
            ignore: IgnoreConfig {
                sources: IgnoreSources::empty(),
                custom_files: vec![PathBuf::from(".ignore")],
                require_git: false,
            },
        },
        ..SearchFilterConfig::default()
    }
}

pub fn scoped_filter(subdir: &str) -> SearchFilterConfig {
    SearchFilterConfig {
        scopes: vec![PathBuf::from(subdir)],
        ..default_filter()
    }
}

// ─── Output helpers ─────────────────────────────────────────────────────────

pub const fn output_std() -> SearchOutput {
    SearchOutput {
        format: SearchOutputFormat::Text,
        mode: SearchMode::Standard,
        emission: OutputEmission::Normal,
        lines: SearchLineStyle {
            filename_mode: FilenameMode::Auto,
            flags: LineStyleFlags::empty(),
            path_display: PathDisplay::Relative,
            max_columns: None,
            max_columns_preview: false,
        },
        records: SearchRecordStyle {
            null_data: false,
            color: ColorChoice::Never,
            path_separator: None,
        },
        passthru: false,
        include_zero: false,
    }
}

pub const fn output_quiet(mode: SearchMode) -> SearchOutput {
    SearchOutput {
        format: SearchOutputFormat::Text,
        mode,
        emission: OutputEmission::Quiet,
        lines: SearchLineStyle {
            filename_mode: FilenameMode::Auto,
            flags: LineStyleFlags::empty(),
            path_display: PathDisplay::Relative,
            max_columns: None,
            max_columns_preview: false,
        },
        records: SearchRecordStyle {
            null_data: false,
            color: ColorChoice::Never,
            path_separator: None,
        },
        passthru: false,
        include_zero: false,
    }
}

pub const fn output_json(mode: SearchMode) -> SearchOutput {
    SearchOutput {
        format: SearchOutputFormat::Json,
        mode,
        emission: OutputEmission::Quiet,
        lines: SearchLineStyle {
            filename_mode: FilenameMode::Auto,
            flags: LineStyleFlags::empty(),
            path_display: PathDisplay::Relative,
            max_columns: None,
            max_columns_preview: false,
        },
        records: SearchRecordStyle {
            null_data: false,
            color: ColorChoice::Never,
            path_separator: None,
        },
        passthru: false,
        include_zero: false,
    }
}

pub fn default_seps() -> SearchSeparators {
    SearchSeparators::default()
}

// ─── Search helpers ─────────────────────────────────────────────────────────

pub fn make_search(patterns: &[&str], opts: SearchOptions) -> CompiledSearch {
    let pats: Vec<String> = patterns.iter().map(ToString::to_string).collect();
    CompiledSearch::new(&pats, opts).unwrap()
}

pub fn make_filter(config: &SearchFilterConfig, root: &Path) -> SearchFilter {
    SearchFilter::new(config, root).unwrap()
}
