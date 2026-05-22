mod common;

use std::fs;

use common::{BuildIndexOptions, assert_success, command, fresh_dir, normalized_stdout};

// ─── --max-depth ─────────────────────────────────────────────────────────────

fn setup_depth_tree(name: &str) -> std::path::PathBuf {
    let root = fresh_dir(name);
    fs::create_dir_all(root.join("a/b/c")).unwrap();
    fs::write(root.join("top.txt"), "hello\n").unwrap();
    fs::write(root.join("a/mid.txt"), "hello\n").unwrap();
    fs::write(root.join("a/b/deep.txt"), "hello\n").unwrap();
    fs::write(root.join("a/b/c/deeper.txt"), "hello\n").unwrap();
    root
}

#[test]
fn max_depth_limits_walk_search() {
    let root = setup_depth_tree("max-depth-walk");
    let missing_idx = fresh_dir("max-depth-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--max-depth")
        .arg("1")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("top.txt"), "should find top.txt");
    assert!(
        stdout.contains("a/mid.txt"),
        "should find a/mid.txt at depth 1"
    );
    assert!(
        !stdout.contains("deep.txt"),
        "should NOT find deep.txt at depth > 1"
    );
    assert!(
        !stdout.contains("deeper.txt"),
        "should NOT find deeper.txt at depth > 1"
    );
}

#[test]
fn max_depth_limits_index_search() {
    let root = setup_depth_tree("max-depth-index");
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--max-depth")
        .arg("1")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("top.txt"), "should find top.txt");
    assert!(
        stdout.contains("a/mid.txt"),
        "should find a/mid.txt at depth 1"
    );
    assert!(
        !stdout.contains("deep.txt"),
        "should NOT find deep.txt at depth > 1"
    );
}

#[test]
fn max_depth_zero_finds_only_root_files() {
    let root = setup_depth_tree("max-depth-zero");
    let missing_idx = fresh_dir("max-depth-zero-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--max-depth")
        .arg("0")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("top.txt"), "should find top.txt at depth 0");
    assert!(
        !stdout.contains("mid.txt"),
        "should NOT find mid.txt at depth 0"
    );
}

// ─── --max-filesize ──────────────────────────────────────────────────────────

fn setup_filesize_tree(name: &str) -> std::path::PathBuf {
    let root = fresh_dir(name);
    fs::write(root.join("small.txt"), "hello\n").unwrap(); // 6 bytes
    fs::write(root.join("big.txt"), "hello\n".repeat(1000)).unwrap(); // 6000 bytes
    root
}

#[test]
fn max_filesize_skips_large_files_walk() {
    let root = setup_filesize_tree("max-filesize-walk");
    let missing_idx = fresh_dir("max-filesize-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--max-filesize")
        .arg("100")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("small.txt"), "should find small.txt");
    assert!(
        !stdout.contains("big.txt"),
        "should skip big.txt above 100 bytes"
    );
}

#[test]
fn max_filesize_skips_large_files_index() {
    let root = setup_filesize_tree("max-filesize-index");
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--max-filesize")
        .arg("100")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("small.txt"), "should find small.txt");
    assert!(
        !stdout.contains("big.txt"),
        "should skip big.txt above 100 bytes"
    );
}

#[test]
fn max_filesize_suffix_k_walk() {
    let root = setup_filesize_tree("max-filesize-k-walk");
    let missing_idx = fresh_dir("max-filesize-k-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--max-filesize")
        .arg("1K")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("small.txt"),
        "should find small.txt under 1K"
    );
    assert!(!stdout.contains("big.txt"), "should skip big.txt above 1K");
}

// ─── --iglob ─────────────────────────────────────────────────────────────────

#[test]
fn iglob_case_insensitive_filter_walk() {
    let root = fresh_dir("iglob-walk");
    fs::write(root.join("file.TXT"), "hello\n").unwrap();
    fs::write(root.join("file.rs"), "hello\n").unwrap();
    let missing_idx = fresh_dir("iglob-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--iglob")
        .arg("*.txt")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("file.TXT"),
        "iglob *.txt should match file.TXT case-insensitively"
    );
    assert!(
        !stdout.contains("file.rs"),
        "iglob *.txt should not match file.rs"
    );
}

#[test]
fn iglob_case_insensitive_filter_index() {
    let root = fresh_dir("iglob-index");
    fs::write(root.join("file.TXT"), "hello\n").unwrap();
    fs::write(root.join("file.rs"), "hello\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--iglob")
        .arg("*.txt")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("file.TXT"),
        "iglob *.txt should match file.TXT case-insensitively"
    );
    assert!(
        !stdout.contains("file.rs"),
        "iglob *.txt should not match file.rs"
    );
}

// ─── --ignore-file ───────────────────────────────────────────────────────────

#[test]
fn ignore_file_custom_walk() {
    let root = fresh_dir("ignore-file-walk");
    fs::write(root.join("a.txt"), "hello\n").unwrap();
    fs::write(root.join("b.log"), "hello\n").unwrap();
    fs::write(root.join("myignore"), "*.log\n").unwrap();
    let missing_idx = fresh_dir("ignore-file-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--ignore-file")
        .arg("myignore")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("a.txt"), "should find a.txt");
    assert!(
        !stdout.contains("b.log"),
        "should skip b.log via custom ignore file"
    );
}

#[test]
fn ignore_file_custom_index() {
    let root = fresh_dir("ignore-file-index");
    fs::write(root.join("a.txt"), "hello\n").unwrap();
    fs::write(root.join("b.log"), "hello\n").unwrap();
    fs::write(root.join("myignore"), "*.log\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--ignore-file")
        .arg("myignore")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("a.txt"), "should find a.txt");
    assert!(
        !stdout.contains("b.log"),
        "should skip b.log via custom ignore file"
    );
}

// ─── --files ─────────────────────────────────────────────────────────────────

#[test]
fn files_lists_matching_paths_walk() {
    let root = fresh_dir("files-walk");
    fs::write(root.join("a.txt"), "content\n").unwrap();
    fs::write(root.join("b.rs"), "content\n").unwrap();
    let missing_idx = fresh_dir("files-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--files")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("a.txt"), "should list a.txt");
    assert!(stdout.contains("b.rs"), "should list b.rs");
}

#[test]
fn files_respects_glob_filter() {
    let root = fresh_dir("files-glob");
    fs::write(root.join("a.txt"), "content\n").unwrap();
    fs::write(root.join("b.rs"), "content\n").unwrap();
    let missing_idx = fresh_dir("files-glob-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--files")
        .arg("-g")
        .arg("*.txt")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("a.txt"), "should list a.txt matching *.txt");
    assert!(!stdout.contains("b.rs"), "should not list b.rs");
}

// ─── --type / --type-not / --type-list / --type-add / --type-clear ───────────

#[test]
fn type_filter_includes_rust_files_walk() {
    let root = fresh_dir("type-include-walk");
    fs::write(root.join("lib.rs"), "hello\n").unwrap();
    fs::write(root.join("script.py"), "hello\n").unwrap();
    fs::write(root.join("notes.txt"), "hello\n").unwrap();
    let missing_idx = fresh_dir("type-include-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("-t")
        .arg("rust")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("lib.rs"), "should find lib.rs with -t rust");
    assert!(
        !stdout.contains("script.py"),
        "should skip script.py with -t rust"
    );
    assert!(
        !stdout.contains("notes.txt"),
        "should skip notes.txt with -t rust"
    );
}

#[test]
fn type_filter_includes_rust_files_index() {
    let root = fresh_dir("type-include-index");
    fs::write(root.join("lib.rs"), "hello\n").unwrap();
    fs::write(root.join("script.py"), "hello\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-t")
        .arg("rust")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("lib.rs"), "should find lib.rs with -t rust");
    assert!(
        !stdout.contains("script.py"),
        "should skip script.py with -t rust"
    );
}

#[test]
fn type_not_excludes_python_walk() {
    let root = fresh_dir("type-not-walk");
    fs::write(root.join("lib.rs"), "hello\n").unwrap();
    fs::write(root.join("script.py"), "hello\n").unwrap();
    let missing_idx = fresh_dir("type-not-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("-T")
        .arg("py")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("lib.rs"), "should find lib.rs with -T py");
    assert!(
        !stdout.contains("script.py"),
        "should skip script.py with -T py"
    );
}

#[test]
fn type_not_excludes_python_index() {
    let root = fresh_dir("type-not-index");
    fs::write(root.join("lib.rs"), "hello\n").unwrap();
    fs::write(root.join("script.py"), "hello\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-T")
        .arg("py")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("lib.rs"), "should find lib.rs with -T py");
    assert!(
        !stdout.contains("script.py"),
        "should skip script.py with -T py"
    );
}

#[test]
fn type_list_output() {
    let out = command(None).arg("--type-list").output().unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("rust: *.rs"), "should list rust type");
    assert!(stdout.contains("py: *.py"), "should list py type");
    assert!(stdout.contains("js: *.js"), "should list js type");
}

#[test]
fn type_add_creates_custom_type() {
    let root = fresh_dir("type-add");
    fs::write(root.join("a.xyz"), "hello\n").unwrap();
    fs::write(root.join("b.txt"), "hello\n").unwrap();
    let missing_idx = fresh_dir("type-add-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--type-add")
        .arg("xyz:*.xyz")
        .arg("-t")
        .arg("xyz")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("a.xyz"),
        "custom type xyz should match a.xyz"
    );
    assert!(
        !stdout.contains("b.txt"),
        "custom type xyz should not match b.txt"
    );
}

#[test]
fn type_clear_removes_builtin() {
    let out = command(None)
        .arg("--type-clear")
        .arg("rust")
        .arg("--type-list")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        !stdout.contains("rust:"),
        "--type-clear rust should remove rust from type list"
    );
    assert!(
        stdout.contains("py:"),
        "other types should still exist after clearing rust"
    );
}

// ─── --sort ──────────────────────────────────────────────────────────────────

#[test]
fn files_output_is_sorted() {
    let root = fresh_dir("files-sorted");
    fs::write(root.join("c.txt"), "content\n").unwrap();
    fs::write(root.join("a.txt"), "content\n").unwrap();
    fs::write(root.join("b.txt"), "content\n").unwrap();
    let missing_idx = fresh_dir("files-sorted-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--files")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    let files: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    let mut sorted = files.clone();
    sorted.sort_unstable();
    assert_eq!(files, sorted, "--files output should be sorted");
}

// ─── compound: --max-depth + --max-filesize ──────────────────────────────────

#[test]
fn compound_max_depth_and_max_filesize_walk() {
    let root = fresh_dir("compound-depth-size-walk");
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("small.txt"), "hello\n").unwrap(); // 6 bytes, depth 0
    fs::write(root.join("big.txt"), "hello\n".repeat(500)).unwrap(); // 3000 bytes, depth 0
    fs::write(root.join("sub/deep.txt"), "hello\n").unwrap(); // 6 bytes, depth 1
    fs::write(root.join("sub/deep_big.txt"), "hello\n".repeat(500)).unwrap(); // 3000 bytes, depth 1
    let missing_idx = fresh_dir("compound-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--max-depth")
        .arg("0")
        .arg("--max-filesize")
        .arg("100")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("small.txt"),
        "should find small.txt (depth 0, small)"
    );
    assert!(
        !stdout.contains("big.txt"),
        "should skip big.txt (too large)"
    );
    assert!(
        !stdout.contains("deep.txt"),
        "should skip deep.txt (too deep)"
    );
    assert!(
        !stdout.contains("deep_big.txt"),
        "should skip deep_big.txt (too deep + too large)"
    );
}

// ─── compound: -t + --type-add ───────────────────────────────────────────────

#[test]
fn type_add_then_include_walk() {
    let root = fresh_dir("type-add-include-walk");
    fs::write(root.join("a.myext"), "hello\n").unwrap();
    fs::write(root.join("b.txt"), "hello\n").unwrap();
    let missing_idx = fresh_dir("type-add-include-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--type-add")
        .arg("custom:*.myext")
        .arg("-t")
        .arg("custom")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains("a.myext"),
        "custom type should match a.myext"
    );
    assert!(
        !stdout.contains("b.txt"),
        "custom type should not match b.txt"
    );
}

// ─── consistency: index vs walk produce same results ─────────────────────────

fn assert_index_walk_filter_consistent(name: &str, extra_args: &[&str]) {
    let root = fresh_dir(name);
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("a.rs"), "hello world\n").unwrap();
    fs::write(root.join("b.py"), "hello world\n").unwrap();
    fs::write(root.join("sub/c.rs"), "hello world\n").unwrap();

    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));
    let missing_idx = fresh_dir(&format!("{name}-noidx")).join(".sift");

    let mut index_cmd = command(Some(&root));
    index_cmd.arg("--sift-dir").arg(&idx);
    for a in extra_args {
        index_cmd.arg(a);
    }
    index_cmd.arg("hello");
    let index_out = index_cmd.output().unwrap();
    assert_success(&index_out);

    let mut walk_cmd = command(Some(&root));
    walk_cmd.arg("--sift-dir").arg(&missing_idx);
    for a in extra_args {
        walk_cmd.arg(a);
    }
    walk_cmd.arg("hello");
    let walk_out = walk_cmd.output().unwrap();
    assert_success(&walk_out);

    let mut index_lines: Vec<String> = normalized_stdout(&index_out)
        .lines()
        .map(str::to_string)
        .collect();
    let mut walk_lines: Vec<String> = normalized_stdout(&walk_out)
        .lines()
        .map(str::to_string)
        .collect();
    index_lines.sort();
    walk_lines.sort();
    assert_eq!(
        index_lines, walk_lines,
        "index and walk should produce same results with args: {extra_args:?}"
    );
}

#[test]
fn type_include_consistent_index_and_walk() {
    assert_index_walk_filter_consistent("type-consistent", &["-t", "rust"]);
}

#[test]
fn max_depth_consistent_index_and_walk() {
    assert_index_walk_filter_consistent("depth-consistent", &["--max-depth", "0"]);
}

#[test]
fn max_filesize_consistent_index_and_walk() {
    assert_index_walk_filter_consistent("filesize-consistent", &["--max-filesize", "10K"]);
}
