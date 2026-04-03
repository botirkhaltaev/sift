mod common;

use std::fs;
use std::process::Command;

use common::{BuildIndexOptions, assert_success, command, fresh_dir, normalized_stdout, rel_match};

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
