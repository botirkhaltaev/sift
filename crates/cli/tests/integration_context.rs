//! Context lines (`-A` / `-B` / `-C`).

mod common;

use std::fs;

use common::{BuildIndexOptions, assert_success, command, fresh_dir, normalized_stdout, rel_match};

#[test]
fn context_c_shows_surrounding_lines() {
    let root = fresh_dir("integration-context-c");
    fs::write(root.join("t.txt"), "alpha\nbeta match\ngamma\n").unwrap();
    let sift_dir = root.join(".sift");
    BuildIndexOptions::default().run(Some(&root), &sift_dir, std::path::Path::new("."));

    let mut cmd = command(Some(&root));
    cmd.arg("--sift-dir").arg(&sift_dir);
    cmd.args(["-C", "1", "match", "t.txt"]);
    let output = cmd.output().unwrap();
    assert_success(&output);

    let expected = format!(
        "{}\n{}\n{}\n",
        rel_match("t.txt", "1-alpha"),
        rel_match("t.txt", "2:beta match"),
        rel_match("t.txt", "3-gamma"),
    );
    assert_eq!(normalized_stdout(&output), expected);
}
