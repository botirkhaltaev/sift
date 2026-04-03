//! `--null` / `-0` and `--color` (smoke).

mod common;

use std::fs;

use common::{BuildIndexOptions, assert_success, command, fresh_dir, normalized_stdout};

#[test]
fn null_terminates_paths_with_files_with_matches() {
    let root = fresh_dir("integration-null-l");
    fs::write(root.join("a.txt"), "needle\n").unwrap();
    fs::write(root.join("b.txt"), "other\n").unwrap();
    let sift_dir = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &sift_dir, std::path::Path::new("."));

    let mut cmd = command(Some(&root));
    cmd.arg("--sift-dir").arg(&sift_dir);
    cmd.args(["-l", "--null", "needle"]);
    let output = cmd.output().unwrap();
    assert_success(&output);

    assert!(
        output.stdout.contains(&b'\0'),
        "expected NUL between path records, got {:?}",
        output.stdout
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains('\n'),
        "with --null, path list should not use newlines"
    );
}

#[test]
fn color_always_emits_ansi_on_stdout() {
    let root = fresh_dir("integration-color");
    fs::write(root.join("t.txt"), "needle\n").unwrap();
    let sift_dir = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &sift_dir, std::path::Path::new("."));

    let mut cmd = command(Some(&root));
    cmd.arg("--sift-dir").arg(&sift_dir);
    cmd.args(["--color=always", "needle", "t.txt"]);
    let output = cmd.output().unwrap();
    assert_success(&output);

    let s = normalized_stdout(&output);
    assert!(
        s.contains('\x1b'),
        "expected ANSI escapes with --color=always, got {s:?}"
    );
}
