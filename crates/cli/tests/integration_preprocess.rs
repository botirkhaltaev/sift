mod common;

use std::process::Command;

use common::{TestProject, assert_exit_code, assert_success, normalize_stderr, rel_match};

#[test]
fn search_zip_reads_gzip_content() {
    if Command::new("gzip").arg("--version").output().is_err() {
        return;
    }

    let p = TestProject::new("search-zip-gzip");
    p.write("hay.txt", "alpha\nneedle\nomega\n");
    let compressed = Command::new("gzip")
        .arg("-c")
        .arg(p.root().join("hay.txt"))
        .output()
        .unwrap();
    assert!(compressed.status.success());
    p.write("hay.txt.gz", compressed.stdout);

    let out = p.walk_output(["--search-zip", "needle", "hay.txt.gz"]);

    assert_success(&out);
    common::assert_stdout_eq(&out, &rel_match("hay.txt.gz", "needle\n"));
}

#[test]
fn indexed_search_zip_bypasses_raw_index_narrowing() {
    if Command::new("gzip").arg("--version").output().is_err() {
        return;
    }

    let p = TestProject::new("indexed-search-zip-gzip");
    p.write("hay.txt", "alpha\nneedle\nomega\n");
    let compressed = Command::new("gzip")
        .arg("-c")
        .arg(p.root().join("hay.txt"))
        .output()
        .unwrap();
    assert!(compressed.status.success());
    p.write("hay.txt.gz", compressed.stdout).build_index();

    let out = p.index_output(["--search-zip", "needle", "hay.txt.gz"]);

    assert_success(&out);
    common::assert_stdout_eq(&out, &rel_match("hay.txt.gz", "needle\n"));
}

#[cfg(unix)]
#[test]
fn preprocessor_stdout_is_searched() {
    use std::os::unix::fs::PermissionsExt;

    let p = TestProject::new("preprocessor-search");
    p.write("hay.txt", "alpha\nneedle\n");
    p.write(
        "upper.sh",
        "#!/bin/sh\ntr '[:lower:]' '[:upper:]' < \"$1\"\n",
    );
    let script = p.root().join("upper.sh");
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();

    let out = p.walk_output(["--pre", script.to_str().unwrap(), "NEEDLE", "hay.txt"]);

    assert_success(&out);
    common::assert_stdout_eq(&out, &rel_match("hay.txt", "NEEDLE\n"));
}

#[cfg(unix)]
#[test]
fn indexed_preprocessor_bypasses_raw_index_narrowing() {
    use std::os::unix::fs::PermissionsExt;

    let p = TestProject::new("indexed-preprocessor-search");
    p.write("hay.txt", "alpha\nneedle\n");
    p.write(
        "upper.sh",
        "#!/bin/sh\ntr '[:lower:]' '[:upper:]' < \"$1\"\n",
    );
    let script = p.root().join("upper.sh");
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    p.build_index();

    let out = p.index_output(["--pre", script.to_str().unwrap(), "NEEDLE", "hay.txt"]);

    assert_success(&out);
    common::assert_stdout_eq(&out, &rel_match("hay.txt", "NEEDLE\n"));
}

#[cfg(unix)]
#[test]
fn pre_glob_limits_preprocessor() {
    use std::os::unix::fs::PermissionsExt;

    let p = TestProject::new("preprocessor-glob");
    p.write("a.txt", "alpha\nneedle\n");
    p.write("b.md", "alpha\nneedle\n");
    p.write(
        "upper.sh",
        "#!/bin/sh\ntr '[:lower:]' '[:upper:]' < \"$1\"\n",
    );
    let script = p.root().join("upper.sh");
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();

    let out = p.walk_output([
        "--pre",
        script.to_str().unwrap(),
        "--pre-glob",
        "*.txt",
        "NEEDLE",
    ]);

    assert_success(&out);
    common::assert_stdout_eq(&out, &rel_match("a.txt", "NEEDLE\n"));
}

#[cfg(unix)]
#[test]
fn failing_preprocessor_reports_error() {
    use std::os::unix::fs::PermissionsExt;

    let p = TestProject::new("preprocessor-failure");
    p.write("hay.txt", "needle\n");
    p.write("fail.sh", "#!/bin/sh\necho nope >&2\nexit 7\n");
    let script = p.root().join("fail.sh");
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();

    let out = p.walk_output(["--pre", script.to_str().unwrap(), "needle", "hay.txt"]);

    assert_exit_code(&out, 2);
    assert!(normalize_stderr(&out).contains("preprocessor"));
}

#[test]
fn invalid_pre_glob_is_reported() {
    let p = TestProject::new("preprocessor-invalid-glob");
    p.write("hay.txt", "needle\n");

    let out = p.walk_output(["--pre", "cat", "--pre-glob", "[", "needle", "hay.txt"]);

    assert_exit_code(&out, 2);
    assert!(normalize_stderr(&out).contains("invalid --pre-glob"));
}
