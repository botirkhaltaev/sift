mod common;

use std::ffi::OsString;
use std::fs;

use common::{
    BuildIndexOptions, abs, assert_index_and_walk_output, assert_success, command, fresh_dir,
    normalized_stdout, rel_match,
};

#[test]
fn quiet_exit_codes() {
    let root = fresh_dir("output-quiet");
    fs::write(root.join("a.txt"), "found\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let ok = command(None)
        .arg("-q")
        .arg("--sift-dir")
        .arg(&idx)
        .arg("found")
        .status()
        .unwrap();
    assert_eq!(ok.code(), Some(0));

    let miss = command(None)
        .arg("-q")
        .arg("--sift-dir")
        .arg(&idx)
        .arg("nopeeee")
        .status()
        .unwrap();
    assert_eq!(miss.code(), Some(1));
}

#[test]
fn files_with_matches_print_each_path_once() {
    let root = fresh_dir("output-files-with-matches");
    fs::write(root.join("a.txt"), "match\nmatch again\n").unwrap();
    fs::write(root.join("b.txt"), "match\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-l")
        .arg("match")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines, ["a.txt".to_string(), "b.txt".to_string()]);
}

#[test]
fn files_without_match_print_only_non_matching_paths() {
    let root = fresh_dir("output-files-without-match");
    fs::write(root.join("a.txt"), "hit\n").unwrap();
    fs::write(root.join("b.txt"), "miss\n").unwrap();
    fs::write(root.join("c.txt"), "hit too\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--files-without-match")
        .arg("hit")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines, ["b.txt".to_string()]);
}

#[test]
fn files_without_match_is_consistent_between_index_and_walk() {
    let root = fresh_dir("output-files-without-match-consistent");
    fs::write(root.join("sherlock.txt"), "Sherlock Holmes\n").unwrap();
    fs::write(root.join("file.py"), "foo\n").unwrap();
    let abs_root = root.canonicalize().unwrap();
    let args = vec![
        OsString::from("--files-without-match"),
        OsString::from("Sherlock"),
        abs_root.into_os_string(),
    ];

    assert_index_and_walk_output(&root, &args, &format!("{}\n", abs(&root, "file.py")));
}

#[test]
fn heading_prints_a_single_file_header() {
    let root = fresh_dir("output-heading");
    fs::write(root.join("sherlock.txt"), "For Sherlock Holmes.\n").unwrap();
    let args = vec![
        OsString::from("-H"),
        OsString::from("--heading"),
        OsString::from("Sherlock"),
        OsString::from("sherlock.txt"),
    ];

    let expected = "sherlock.txt\nFor Sherlock Holmes.\n";
    assert_index_and_walk_output(&root, &args, expected);
}

#[test]
fn no_heading_overrides_heading() {
    let root = fresh_dir("output-no-heading");
    fs::write(root.join("sherlock.txt"), "For Sherlock Holmes.\n").unwrap();
    let args = vec![
        OsString::from("-H"),
        OsString::from("--heading"),
        OsString::from("--no-heading"),
        OsString::from("Sherlock"),
        OsString::from("sherlock.txt"),
    ];

    let expected = format!("{}\n", rel_match("sherlock.txt", "For Sherlock Holmes."));
    assert_index_and_walk_output(&root, &args, &expected);
}

#[test]
fn count_prints_match_totals_per_file() {
    let root = fresh_dir("output-count");
    fs::write(root.join("a.txt"), "hit\nhit\n").unwrap();
    fs::write(root.join("b.txt"), "miss\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-c")
        .arg("hit")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines, [rel_match("a.txt", "2")]);
}

#[test]
fn line_number_and_no_filename_format_output() {
    let root = fresh_dir("output-line-number-no-filename");
    fs::write(root.join("t.txt"), "alpha\nbeta\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-n")
        .arg("--no-filename")
        .arg("beta")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines, ["2:beta"]);
}

#[test]
fn only_matching_prints_each_match_span() {
    let root = fresh_dir("output-only-matching");
    fs::write(root.join("t.txt"), "alpha beta beta\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-o")
        .arg("--no-filename")
        .arg("beta")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines, ["beta", "beta"]);
}

#[test]
fn max_count_limits_per_file() {
    let root = fresh_dir("output-max-count");
    fs::write(root.join("a.txt"), "match one\nmatch two\n").unwrap();
    fs::write(root.join("b.txt"), "match three\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--max-count")
        .arg("1")
        .arg("--no-filename")
        .arg("match")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 2, "expected 1 line per file: {lines:?}");
    assert_eq!(lines, &["match one", "match three"]);
}

#[test]
fn count_matches_counts_individual_spans() {
    let root = fresh_dir("output-count-matches");
    fs::write(root.join("a.txt"), "beta beta beta\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--count-matches")
        .arg("beta")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines, &[rel_match("a.txt", "3")]);
}

#[test]
fn count_lines_not_matches() {
    let root = fresh_dir("output-count-lines");
    fs::write(root.join("a.txt"), "beta beta beta\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-c")
        .arg("beta")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines, &[rel_match("a.txt", "1")]);
}

#[test]
fn c_o_normalizes_to_count_matches() {
    let root = fresh_dir("output-c-o");
    fs::write(root.join("a.txt"), "beta beta\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    for args in &[&["-c", "-o"][..], &["-o", "-c"][..]] {
        let out = command(None)
            .arg("--sift-dir")
            .arg(&idx)
            .args(*args)
            .arg("--no-filename")
            .arg("beta")
            .output()
            .unwrap();
        assert_success(&out);
        assert_eq!(
            normalized_stdout(&out).trim(),
            "2",
            "-c -o should count individual matches"
        );
    }
}

#[test]
fn count_matches_quiet_match() {
    let root = fresh_dir("output-count-matches-quiet");
    fs::write(root.join("a.txt"), "beta beta\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--count-matches")
        .arg("-q")
        .arg("beta")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(normalized_stdout(&out).is_empty());
}

#[test]
fn count_matches_quiet_no_match() {
    let root = fresh_dir("output-count-matches-quiet-nomatch");
    fs::write(root.join("a.txt"), "beta beta\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--count-matches")
        .arg("-q")
        .arg("notfound")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(normalized_stdout(&out).is_empty());
}

#[test]
fn count_matches_no_filename() {
    let root = fresh_dir("output-count-matches-no-filename");
    fs::write(root.join("a.txt"), "beta beta\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--count-matches")
        .arg("--no-filename")
        .arg("beta")
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(normalized_stdout(&out).trim(), "2");
}

#[test]
fn count_omits_zero_count_files() {
    let root = fresh_dir("output-count-omit-zero");
    fs::write(root.join("a.txt"), "hit\n").unwrap();
    fs::write(root.join("b.txt"), "miss\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-c")
        .arg("hit")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines, &[rel_match("a.txt", "1")]);
}

#[test]
fn count_matches_multi_line() {
    let root = fresh_dir("output-count-matches-multi");
    fs::write(root.join("a.txt"), "a a a\nx\na\n").unwrap();
    let idx = root.join(".sift");

    BuildIndexOptions::default().run(None, &idx, &root);

    let out = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("--count-matches")
        .arg("a")
        .output()
        .unwrap();
    assert_success(&out);

    let lines: Vec<_> = normalized_stdout(&out)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines, &[rel_match("a.txt", "4")]);

    let out_c = command(None)
        .arg("--sift-dir")
        .arg(&idx)
        .arg("-c")
        .arg("a")
        .output()
        .unwrap();
    assert_success(&out_c);

    let lines_c: Vec<_> = normalized_stdout(&out_c)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines_c.len(), 1);
    assert_eq!(lines_c, &[rel_match("a.txt", "2")]);
}
