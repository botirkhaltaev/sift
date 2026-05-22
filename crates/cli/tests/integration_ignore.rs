mod common;

use std::fs;
use std::process::Command;

use common::{BuildIndexOptions, assert_success, command, fresh_dir, normalized_stdout, rel_match};

// ─── existing ignore tests ───────────────────────────────────────────────────

#[test]
fn gitignore_excludes_when_git_present() {
    let root = fresh_dir("ignore-with-git");
    fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    fs::write(root.join("keep.txt"), "needle\n").unwrap();
    fs::write(root.join("skip.log"), "needle\n").unwrap();
    let status = Command::new("git")
        .args(["init"])
        .current_dir(&root)
        .status()
        .expect("git binary for ignore test");
    assert!(status.success(), "git init failed");

    let idx = root.join(".sift");
    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("needle")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(
        stdout.contains(&rel_match("keep.txt", "needle")),
        "expected keep.txt: {stdout}"
    );
    assert!(
        !stdout.contains("skip.log"),
        "gitignored file should be skipped: {stdout}"
    );
}

#[test]
fn no_require_git_loads_gitignore_without_dot_git() {
    let root = fresh_dir("ignore-no-git-repo");
    fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    fs::write(root.join("keep.txt"), "needle\n").unwrap();
    fs::write(root.join("skip.log"), "needle\n").unwrap();
    assert!(!root.join(".git").exists());

    let idx = root.join(".sift");
    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--no-require-git")
        .arg("needle")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains(&rel_match("keep.txt", "needle")));
    assert!(!stdout.contains("skip.log"), "unexpected: {stdout}");
}

#[test]
fn require_git_skips_gitignore_when_no_git_dir() {
    let root = fresh_dir("ignore-require-git-no-repo");
    fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    fs::write(root.join("keep.txt"), "needle\n").unwrap();
    fs::write(root.join("skip.log"), "needle\n").unwrap();
    assert!(!root.join(".git").exists());

    let idx = root.join(".sift");
    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("needle")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains(&rel_match("keep.txt", "needle")));
    assert!(
        stdout.contains(&rel_match("skip.log", "needle")),
        "without .git, default require_git skips loading .gitignore: {stdout}"
    );
}

#[test]
fn no_ignore_disables_gitignore() {
    let root = fresh_dir("ignore-no-ignore-flag");
    fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    fs::write(root.join("keep.txt"), "needle\n").unwrap();
    fs::write(root.join("skip.log"), "needle\n").unwrap();
    let status = Command::new("git")
        .args(["init"])
        .current_dir(&root)
        .status()
        .expect("git");
    assert!(status.success());

    let idx = root.join(".sift");
    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--no-ignore")
        .arg("needle")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains(&rel_match("keep.txt", "needle")));
    assert!(
        stdout.contains(&rel_match("skip.log", "needle")),
        "--no-ignore should search ignored paths: {stdout}"
    );
}

// ─── --no-ignore-parent ──────────────────────────────────────────────────────

#[test]
fn no_ignore_parent_walk() {
    let parent = fresh_dir("no-ignore-parent-walk");
    fs::write(parent.join(".gitignore"), "*.log\n").unwrap();
    let child = parent.join("project");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir_all(child.join(".git")).unwrap();
    fs::write(child.join("data.log"), "findme in log\n").unwrap();
    fs::write(child.join("data.txt"), "findme in txt\n").unwrap();
    let missing_idx = fresh_dir("no-ignore-parent-walk-noidx").join(".sift");

    // Without --no-ignore-parent: parent .gitignore applies, data.log excluded
    let out = command(Some(&child))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("data.txt"), "should find txt");

    // With --no-ignore-parent: parent .gitignore NOT applied, data.log found
    let missing_idx2 = fresh_dir("no-ignore-parent-walk2-noidx").join(".sift");
    let out = command(Some(&child))
        .arg("--sift-dir")
        .arg(&missing_idx2)
        .arg("--no-ignore-parent")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("data.txt"), "should find txt");
    assert!(
        stdout.contains("data.log"),
        "with --no-ignore-parent, should find log file"
    );
}

// ─── --no-ignore-exclude ─────────────────────────────────────────────────────

#[test]
fn no_ignore_exclude_walk() {
    let root = fresh_dir("no-ignore-exclude-walk");
    fs::create_dir_all(root.join(".git/info")).unwrap();
    fs::write(root.join(".git/info/exclude"), "*.bak\n").unwrap();
    fs::write(root.join("file.bak"), "findme in bak\n").unwrap();
    fs::write(root.join("file.txt"), "findme in txt\n").unwrap();
    let missing_idx = fresh_dir("no-ignore-exclude-walk-noidx").join(".sift");

    // With exclude: .bak is ignored
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("file.txt"), "should find txt");
    assert!(!stdout.contains("file.bak"), "exclude should hide .bak");

    // With --no-ignore-exclude: .bak is found
    let missing_idx2 = fresh_dir("no-ignore-exclude-walk2-noidx").join(".sift");
    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx2)
        .arg("--no-ignore-exclude")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("file.txt"), "should find txt");
    assert!(
        stdout.contains("file.bak"),
        "with --no-ignore-exclude, should find .bak"
    );
}

#[test]
fn no_ignore_exclude_index() {
    let root = fresh_dir("no-ignore-exclude-index");
    fs::create_dir_all(root.join(".git/info")).unwrap();
    fs::write(root.join(".git/info/exclude"), "*.bak\n").unwrap();
    fs::write(root.join("file.bak"), "findme in bak\n").unwrap();
    fs::write(root.join("file.txt"), "findme in txt\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--no-ignore-exclude")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("file.txt"), "should find txt");
    assert!(
        stdout.contains("file.bak"),
        "with --no-ignore-exclude, should find .bak in index mode"
    );
}

// ─── --ignore-file / --no-ignore-files ───────────────────────────────────────

#[test]
fn ignore_file_walk() {
    let root = fresh_dir("ignore-file-walk");
    fs::write(root.join("secret.txt"), "findme secret\n").unwrap();
    fs::write(root.join("public.txt"), "findme public\n").unwrap();
    let custom_ignore = root.join("custom.ignore");
    fs::write(&custom_ignore, "secret.txt\n").unwrap();
    let missing_idx = fresh_dir("ignore-file-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--ignore-file")
        .arg(&custom_ignore)
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("public.txt"), "should find public");
    assert!(
        !stdout.contains("secret.txt"),
        "ignore-file should hide secret"
    );
}

#[test]
fn ignore_file_index() {
    let root = fresh_dir("ignore-file-index");
    fs::write(root.join("secret.txt"), "findme secret\n").unwrap();
    fs::write(root.join("public.txt"), "findme public\n").unwrap();
    let custom_ignore = root.join("custom.ignore");
    fs::write(&custom_ignore, "secret.txt\n").unwrap();
    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--ignore-file")
        .arg(&custom_ignore)
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("public.txt"), "should find public");
    assert!(
        !stdout.contains("secret.txt"),
        "ignore-file should hide secret in index mode"
    );
}

#[test]
fn no_ignore_files_overrides_ignore_file() {
    let root = fresh_dir("no-ignore-files-walk");
    fs::write(root.join("secret.txt"), "findme secret\n").unwrap();
    fs::write(root.join("public.txt"), "findme public\n").unwrap();
    let custom_ignore = root.join("custom.ignore");
    fs::write(&custom_ignore, "secret.txt\n").unwrap();
    let missing_idx = fresh_dir("no-ignore-files-walk-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--no-ignore-files")
        .arg("--ignore-file")
        .arg(&custom_ignore)
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("public.txt"), "should find public");
    assert!(
        stdout.contains("secret.txt"),
        "--no-ignore-files should override --ignore-file"
    );
}

#[test]
fn ignore_file_consistent_index_and_walk() {
    let root = fresh_dir("ignore-file-consistent");
    fs::write(root.join("a.txt"), "findme alpha\n").unwrap();
    fs::write(root.join("b.txt"), "findme beta\n").unwrap();
    let custom_ignore = root.join("custom.ignore");
    fs::write(&custom_ignore, "b.txt\n").unwrap();

    let idx = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &idx, std::path::Path::new("."));
    let missing_idx = fresh_dir("ignore-file-consistent-noidx").join(".sift");

    let index_out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--ignore-file")
        .arg(&custom_ignore)
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&index_out);

    let walk_out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--ignore-file")
        .arg(&custom_ignore)
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&walk_out);

    let mut index_lines: Vec<String> = normalized_stdout(&index_out)
        .lines()
        .map(str::to_string)
        .collect();
    let mut walk_lines: Vec<String> = normalized_stdout(&walk_out)
        .lines()
        .map(str::to_string)
        .collect();
    index_lines.sort_unstable();
    walk_lines.sort_unstable();
    assert_eq!(
        index_lines, walk_lines,
        "ignore-file should produce same results in both modes"
    );
}

// ─── --no-messages ───────────────────────────────────────────────────────────

#[test]
fn no_messages_suppresses_error_output() {
    let root = fresh_dir("no-messages");
    fs::write(root.join("file.txt"), "hello\n").unwrap();
    let missing_idx = fresh_dir("no-messages-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--no-messages")
        .arg("hello")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("error: unexpected argument"),
        "--no-messages should be accepted as a valid flag"
    );
}

// ─── --no-ignore-messages ────────────────────────────────────────────────────

#[test]
fn no_ignore_messages_accepted() {
    let root = fresh_dir("no-ignore-messages");
    fs::write(root.join("file.txt"), "hello\n").unwrap();
    let missing_idx = fresh_dir("no-ignore-messages-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--no-ignore-messages")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("hello"), "should find match");
}

// ─── --no-ignore-global ──────────────────────────────────────────────────────

#[test]
fn no_ignore_global_accepted() {
    let root = fresh_dir("no-ignore-global");
    fs::write(root.join("file.txt"), "hello\n").unwrap();
    let missing_idx = fresh_dir("no-ignore-global-noidx").join(".sift");

    let out = command(Some(&root))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--no-ignore-global")
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("hello"), "should find match");
}

// ─── toggling: --no-ignore-parent then --ignore-parent ───────────────────────

#[test]
fn ignore_parent_toggle_last_wins() {
    let parent = fresh_dir("ignore-parent-toggle");
    fs::write(parent.join(".gitignore"), "*.log\n").unwrap();
    let child = parent.join("project");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir_all(child.join(".git")).unwrap();
    fs::write(child.join("data.log"), "findme in log\n").unwrap();
    fs::write(child.join("data.txt"), "findme in txt\n").unwrap();
    let missing_idx = fresh_dir("ignore-parent-toggle-noidx").join(".sift");

    let out = command(Some(&child))
        .arg("--sift-dir")
        .arg(&missing_idx)
        .arg("--no-ignore-parent")
        .arg("--ignore-parent")
        .arg("findme")
        .output()
        .unwrap();
    assert_success(&out);
    let stdout = normalized_stdout(&out);
    assert!(stdout.contains("data.txt"), "should find txt");
    assert!(
        !stdout.contains("data.log"),
        "--ignore-parent should re-apply parent rules"
    );
}
