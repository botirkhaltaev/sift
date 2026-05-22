//! Context lines (`-A` / `-B` / `-C`) and separator flags.

mod common;

use common::{TestProject, assert_success, normalize_stdout};

#[test]
fn context_c_shows_surrounding_lines() {
    let p = TestProject::new("context-c");
    p.write("t.txt", "alpha\nbeta match\ngamma\n");
    p.build_index();
    let out = p.index_output(["-C", "1", "match", "t.txt"]);
    assert_success(&out);
    assert_eq!(
        normalize_stdout(&out),
        "t.txt-1-alpha\nt.txt:2:beta match\nt.txt-3-gamma\n"
    );
}

#[test]
fn context_a_shows_lines_after_match() {
    let p = TestProject::new("context-a");
    p.write("t.txt", "alpha\nbeta match\ngamma\ndelta\n");
    p.build_index();
    let out = p.index_output(["-A", "2", "match", "t.txt"]);
    assert_success(&out);
    assert_eq!(
        normalize_stdout(&out),
        "t.txt:2:beta match\nt.txt-3-gamma\nt.txt-4-delta\n"
    );
}

#[test]
fn context_b_shows_lines_before_match() {
    let p = TestProject::new("context-b");
    p.write("t.txt", "alpha\nbeta match\ngamma\n");
    p.build_index();
    let out = p.index_output(["-B", "2", "match", "t.txt"]);
    assert_success(&out);
    let stdout = normalize_stdout(&out);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "expected 2 lines (1 before + match), got: {lines:?}"
    );
    assert!(
        lines[0].contains("alpha"),
        "expected 'alpha' in line 1, got: {}",
        lines[0]
    );
    assert!(
        lines[1].contains("match"),
        "expected 'match' in line 2, got: {}",
        lines[1]
    );
}

#[test]
fn context_break_separates_match_groups() {
    let p = TestProject::new("context-break");
    p.write(
        "t.txt",
        "line1 match\nline2 not\nline3 not\nline4 not\nline5 match\nline6 not\nline7 not\nline8 match\n",
    );
    p.build_index();
    let out = p.index_output(["-B", "1", "-A", "1", "match", "t.txt"]);
    assert_success(&out);
    let stdout = normalize_stdout(&out);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 7,
        "expected at least 7 lines, got {}: {lines:?}",
        lines.len()
    );
}

#[test]
fn context_c_with_filename_uses_hyphen_separator() {
    let p = TestProject::new("context-filename");
    p.write("t.txt", "alpha\nbeta match\ngamma\n");
    p.build_index();
    let out = p.index_output(["-n", "-C", "1", "match", "t.txt"]);
    assert_success(&out);
    let stdout = normalize_stdout(&out);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].starts_with("t.txt-1-"),
        "expected 't.txt-1-' prefix for context line, got: {}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("t.txt:2:"),
        "expected 't.txt:2:' prefix for match line, got: {}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("t.txt-3-"),
        "expected 't.txt-3-' prefix for context line, got: {}",
        lines[2]
    );
}

// ─── --context-separator ─────────────────────────────────────────────────────

#[test]
fn custom_context_separator_index_and_walk() {
    let p = TestProject::new("ctx-sep-custom");
    p.write(
        "t.txt",
        "m1 match\nfiller\nfiller\nfiller\nfiller\nm2 match\n",
    );
    let expected = "t.txt:1:m1 match\nt.txt-2-filler\n===\nt.txt-5-filler\nt.txt:6:m2 match\n";
    p.assert_index_walk_same(
        &[
            "-n",
            "-C",
            "1",
            "--context-separator",
            "===",
            "match",
            "t.txt",
        ],
        expected,
    );
}

#[test]
fn no_context_separator_suppresses_break_line() {
    let p = TestProject::new("ctx-sep-none");
    p.write(
        "t.txt",
        "m1 match\nfiller\nfiller\nfiller\nfiller\nm2 match\n",
    );
    let expected = "t.txt:1:m1 match\nt.txt-2-filler\nt.txt-5-filler\nt.txt:6:m2 match\n";
    p.assert_index_walk_same(
        &["-n", "-C", "1", "--no-context-separator", "match", "t.txt"],
        expected,
    );
}

#[test]
fn context_separator_empty_string_prints_blank_line() {
    let p = TestProject::new("ctx-sep-empty");
    p.write(
        "t.txt",
        "m1 match\nfiller\nfiller\nfiller\nfiller\nm2 match\n",
    );
    let expected = "t.txt:1:m1 match\nt.txt-2-filler\n\nt.txt-5-filler\nt.txt:6:m2 match\n";
    p.assert_index_walk_same(
        &["-n", "-C", "1", "--context-separator", "", "match", "t.txt"],
        expected,
    );
}

#[test]
fn context_separator_with_escape_sequences() {
    let p = TestProject::new("ctx-sep-escape");
    p.write(
        "t.txt",
        "m1 match\nfiller\nfiller\nfiller\nfiller\nm2 match\n",
    );
    let expected = "t.txt:1:m1 match\nt.txt-2-filler\n---\n---\nt.txt-5-filler\nt.txt:6:m2 match\n";
    p.assert_index_walk_same(
        &[
            "-n",
            "-C",
            "1",
            "--context-separator",
            "---\\n---",
            "match",
            "t.txt",
        ],
        expected,
    );
}

// ─── --field-match-separator ─────────────────────────────────────────────────

#[test]
fn field_match_separator_index_and_walk() {
    let p = TestProject::new("field-match-sep");
    p.write("t.txt", "hello world\n");
    let expected = "t.txt=1=hello world\n";
    p.assert_index_walk_same(
        &["-n", "--field-match-separator", "=", "hello", "t.txt"],
        expected,
    );
}

// ─── --field-context-separator ───────────────────────────────────────────────

#[test]
fn field_context_separator_index_and_walk() {
    let p = TestProject::new("field-ctx-sep");
    p.write("t.txt", "alpha\nbeta match\ngamma\n");
    let expected = "t.txt~1~alpha\nt.txt:2:beta match\nt.txt~3~gamma\n";
    p.assert_index_walk_same(
        &[
            "-n",
            "-C",
            "1",
            "--field-context-separator",
            "~",
            "match",
            "t.txt",
        ],
        expected,
    );
}

#[test]
fn field_match_and_context_separator_combined() {
    let p = TestProject::new("field-both-sep");
    p.write("t.txt", "alpha\nbeta match\ngamma\n");
    let expected = "t.txt~1~alpha\nt.txt|2|beta match\nt.txt~3~gamma\n";
    p.assert_index_walk_same(
        &[
            "-n",
            "-C",
            "1",
            "--field-match-separator",
            "|",
            "--field-context-separator",
            "~",
            "match",
            "t.txt",
        ],
        expected,
    );
}

#[test]
fn all_separator_flags_combined() {
    let p = TestProject::new("all-sep-combined");
    p.write(
        "t.txt",
        "m1 match\nfiller\nfiller\nfiller\nfiller\nm2 match\ngamma\n",
    );
    let expected =
        "t.txt|1|m1 match\nt.txt~2~filler\n***\nt.txt~5~filler\nt.txt|6|m2 match\nt.txt~7~gamma\n";
    p.assert_index_walk_same(
        &[
            "-n",
            "-C",
            "1",
            "--context-separator",
            "***",
            "--field-match-separator",
            "|",
            "--field-context-separator",
            "~",
            "match",
            "t.txt",
        ],
        expected,
    );
}

#[test]
fn no_context_separator_overrides_context_separator() {
    let p = TestProject::new("ctx-sep-override");
    p.write(
        "t.txt",
        "m1 match\nfiller\nfiller\nfiller\nfiller\nm2 match\n",
    );
    let expected = "t.txt:1:m1 match\nt.txt-2-filler\nt.txt-5-filler\nt.txt:6:m2 match\n";
    p.assert_index_walk_same(
        &[
            "-n",
            "-C",
            "1",
            "--context-separator",
            "===",
            "--no-context-separator",
            "match",
            "t.txt",
        ],
        expected,
    );
}
