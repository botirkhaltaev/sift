mod common;

use std::ffi::OsString;

use common::{TestProject, abs_path, assert_success, normalize_stdout, rel_match};

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  Exit codes
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn quiet_exit_codes() {
    let p = TestProject::new("output-quiet");
    p.write("a.txt", "found\n");
    p.build_index_at(p.root());

    let ok = p.index_status(["-q", "found"]);
    assert_eq!(ok.code(), Some(0));

    let miss = p.index_status(["-q", "nopeeee"]);
    assert_eq!(miss.code(), Some(1));
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --files-with-matches / -l
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn files_with_matches_print_each_path_once() {
    let p = TestProject::new("output-files-with-matches");
    p.write("a.txt", "match\nmatch again\n");
    p.write("b.txt", "match\n");
    p.build_index();

    let out = p.index_output(["-l", "match"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines, ["a.txt".to_string(), "b.txt".to_string()]);
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --files-without-match
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn files_without_match_print_only_non_matching_paths() {
    let p = TestProject::new("output-files-without-match");
    p.write("a.txt", "hit\n");
    p.write("b.txt", "miss\n");
    p.write("c.txt", "hit too\n");
    p.build_index();

    let out = p.index_output(["--files-without-match", "hit"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines, ["b.txt".to_string()]);
}

#[test]
fn files_without_match_is_consistent_between_index_and_walk() {
    let p = TestProject::new("output-files-without-match-consistent");
    p.write("sherlock.txt", "Sherlock Holmes\n");
    p.write("file.py", "foo\n");
    let args = [
        OsString::from("--files-without-match"),
        OsString::from("Sherlock"),
        p.root().canonicalize().unwrap().into(),
    ];
    let expected = format!("{}\n", abs_path(p.root(), "file.py"));
    p.assert_index_walk_same(args, &expected);
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --heading / -H
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn heading_prints_a_single_file_header() {
    let p = TestProject::new("output-heading");
    p.write("sherlock.txt", "For Sherlock Holmes.\n");
    p.assert_index_walk_same(
        &["-H", "--heading", "Sherlock", "sherlock.txt"],
        "sherlock.txt\nFor Sherlock Holmes.\n",
    );
}

#[test]
fn no_heading_overrides_heading() {
    let p = TestProject::new("output-no-heading");
    p.write("sherlock.txt", "For Sherlock Holmes.\n");
    let expected = format!("{}\n", rel_match("sherlock.txt", "For Sherlock Holmes."));
    p.assert_index_walk_same(
        &[
            "-H",
            "--heading",
            "--no-heading",
            "Sherlock",
            "sherlock.txt",
        ],
        &expected,
    );
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --count / -c
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn count_prints_match_totals_per_file() {
    let p = TestProject::new("output-count");
    p.write("a.txt", "hit\nhit\n");
    p.write("b.txt", "miss\n");
    p.build_index();

    let out = p.index_output(["-c", "hit"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines, [rel_match("a.txt", "2")]);
}

#[test]
fn count_lines_not_matches() {
    let p = TestProject::new("output-count-lines");
    p.write("a.txt", "beta beta beta\n");
    p.build_index();

    let out = p.index_output(["-c", "beta"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines, &[rel_match("a.txt", "1")]);
}

#[test]
fn count_omits_zero_count_files() {
    let p = TestProject::new("output-count-omit-zero");
    p.write("a.txt", "hit\n");
    p.write("b.txt", "miss\n");
    p.build_index();

    let out = p.index_output(["-c", "hit"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines, &[rel_match("a.txt", "1")]);
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --count-matches
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn count_matches_counts_individual_spans() {
    let p = TestProject::new("output-count-matches");
    p.write("a.txt", "beta beta beta\n");
    p.build_index();

    let out = p.index_output(["--count-matches", "beta"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines, &[rel_match("a.txt", "3")]);
}

#[test]
fn count_matches_multi_line() {
    let p = TestProject::new("output-count-matches-multi");
    p.write("a.txt", "a a a\nx\na\n");
    p.build_index();

    let out = p.index_output(["--count-matches", "a"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines, &[rel_match("a.txt", "4")]);

    let out_c = p.index_output(["-c", "a"]);
    assert_success(&out_c);
    let lines_c: Vec<_> = normalize_stdout(&out_c)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines_c.len(), 1);
    assert_eq!(lines_c, &[rel_match("a.txt", "2")]);
}

#[test]
fn count_matches_quiet_match() {
    let p = TestProject::new("output-count-matches-quiet");
    p.write("a.txt", "beta beta\n");
    p.build_index();

    let out = p.index_output(["--count-matches", "-q", "beta"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(normalize_stdout(&out).is_empty());
}

#[test]
fn count_matches_quiet_no_match() {
    let p = TestProject::new("output-count-matches-quiet-nomatch");
    p.write("a.txt", "beta beta\n");
    p.build_index();

    let out = p.index_output(["--count-matches", "-q", "notfound"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(normalize_stdout(&out).is_empty());
}

#[test]
fn count_matches_no_filename() {
    let p = TestProject::new("output-count-matches-no-filename");
    p.write("a.txt", "beta beta\n");
    p.build_index();

    let out = p.index_output(["--count-matches", "--no-filename", "beta"]);
    assert_success(&out);
    assert_eq!(normalize_stdout(&out).trim(), "2");
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --line-number / -n
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn line_number_and_no_filename_format_output() {
    let p = TestProject::new("output-line-number-no-filename");
    p.write("t.txt", "alpha\nbeta\n");
    p.build_index();

    let out = p.index_output(["-n", "--no-filename", "beta"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines, ["2:beta"]);
}

#[test]
fn no_line_number_suppresses_line_numbers() {
    let p = TestProject::new("output-no-line");
    p.write("a.txt", "hello\nworld\n");
    p.build_index();

    let out = p.index_output(["-n", "-N", "hello"]);
    assert_success(&out);
    assert_eq!(normalize_stdout(&out).trim(), "a.txt:hello");
}

#[test]
fn no_line_number_long_form() {
    let p = TestProject::new("output-no-line-long");
    p.write("a.txt", "hello\n");
    p.build_index();

    let out = p.index_output(["--line-number", "--no-line-number", "hello"]);
    assert_success(&out);
    assert_eq!(normalize_stdout(&out).trim(), "a.txt:hello");
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --only-matching / -o
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn only_matching_prints_each_match_span() {
    let p = TestProject::new("output-only-matching");
    p.write("t.txt", "alpha beta beta\n");
    p.build_index();

    let out = p.index_output(["-o", "--no-filename", "beta"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines, ["beta", "beta"]);
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --max-count
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn max_count_limits_per_file() {
    let p = TestProject::new("output-max-count");
    p.write("a.txt", "match one\nmatch two\n");
    p.write("b.txt", "match three\n");
    p.build_index();

    let out = p.index_output(["--max-count", "1", "--no-filename", "match"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 2, "expected 1 line per file: {lines:?}");
    assert_eq!(lines, &["match one", "match three"]);
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  -c -o interaction
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn c_o_normalizes_to_count_matches() {
    let p = TestProject::new("output-c-o");
    p.write("a.txt", "beta beta\n");
    p.build_index();

    for variant in [["-c", "-o"], ["-o", "-c"]] {
        let out = p.index_output(variant.into_iter().chain(["--no-filename", "beta"]));
        assert_success(&out);
        assert_eq!(
            normalize_stdout(&out).trim(),
            "2",
            "-c -o should count individual matches"
        );
    }
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --column
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn column_shows_1_based_byte_offset() {
    let p = TestProject::new("output-column");
    p.write("a.txt", "hello world\n");
    p.build_index();

    let out = p.index_output(["--column", "-n", "world"]);
    assert_success(&out);
    assert_eq!(normalize_stdout(&out).trim(), "a.txt:1:7:hello world");
}

#[test]
fn column_walk_mode() {
    let p = TestProject::new("output-column-walk");
    p.write("a.txt", "hello world\n");
    let out = p.walk_output(["--column", "-n", "world"]);
    assert_success(&out);
    assert_eq!(normalize_stdout(&out).trim(), "a.txt:1:7:hello world");
}

#[test]
fn column_only_matching() {
    let p = TestProject::new("output-column-om");
    p.write("a.txt", "aXbXc\n");
    p.build_index();

    let out = p.index_output(["--column", "-o", "-n", "X"]);
    assert_success(&out);
    let lines: Vec<_> = normalize_stdout(&out).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "a.txt:1:2:X");
    assert_eq!(lines[1], "a.txt:1:4:X");
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --vimgrep
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn vimgrep_implies_line_and_column() {
    let p = TestProject::new("output-vimgrep");
    p.write("a.txt", "hello world\n");
    p.build_index();

    let out = p.index_output(["--vimgrep", "world"]);
    assert_success(&out);
    assert_eq!(normalize_stdout(&out).trim(), "a.txt:1:7:hello world");
}

#[test]
fn vimgrep_walk_mode() {
    let p = TestProject::new("output-vimgrep-walk");
    p.write("a.txt", "hello world\n");
    let out = p.walk_output(["--vimgrep", "world"]);
    assert_success(&out);
    assert_eq!(normalize_stdout(&out).trim(), "a.txt:1:7:hello world");
}

#[test]
fn vimgrep_consistent_index_and_walk() {
    let p = TestProject::new("output-vimgrep-consistent");
    p.write("a.txt", "hello world\n");
    p.build_index();
    let index_out = p.index_output(["--vimgrep", "world"]);
    let walk_out = p.walk_output(["--vimgrep", "world"]);
    assert_success(&index_out);
    assert_success(&walk_out);
    assert_eq!(
        normalize_stdout(&index_out).trim(),
        normalize_stdout(&walk_out).trim(),
        "index and walk --vimgrep results differ"
    );
    assert_eq!(normalize_stdout(&index_out).trim(), "a.txt:1:7:hello world");
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --pretty / -p
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn pretty_enables_heading_and_line_numbers() {
    let p = TestProject::new("output-pretty");
    p.write("a.txt", "hello\n");
    p.build_index();

    let out = p.index_output(["-p", "--color", "never", "hello"]);
    assert_success(&out);
    let stdout = normalize_stdout(&out);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected heading + match, got: {lines:?}");
    assert_eq!(lines[0], "a.txt");
    assert_eq!(lines[1], "1:hello");
}

#[test]
fn pretty_walk_mode() {
    let p = TestProject::new("output-pretty-walk");
    p.write("a.txt", "hello\n");
    let out = p.walk_output(["-p", "--color", "never", "hello"]);
    assert_success(&out);
    let stdout = normalize_stdout(&out);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected heading + match, got: {lines:?}");
    assert_eq!(lines[0], "a.txt");
    assert_eq!(lines[1], "1:hello");
}

// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──
//  --version / -V
// ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──

#[test]
fn version_flag_prints_version() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_sift"))
        .arg("--version")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(
        normalize_stdout(&out).contains("sift-grep") || normalize_stdout(&out).contains("sift"),
        "expected version string"
    );
}

#[test]
fn version_short_flag() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_sift"))
        .arg("-V")
        .output()
        .unwrap();
    assert_success(&out);
    assert!(
        normalize_stdout(&out).contains("sift-grep") || normalize_stdout(&out).contains("sift"),
        "expected version string"
    );
}
