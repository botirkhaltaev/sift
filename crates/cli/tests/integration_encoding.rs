mod common;

use common::TestProject;

fn utf16le_with_bom(s: &str) -> Vec<u8> {
    let mut out = vec![0xff, 0xfe];
    for unit in s.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

#[test]
fn utf16_bom_is_searched_by_default() {
    let p = TestProject::new("integration-encoding-utf16-bom");
    p.write("utf16.txt", utf16le_with_bom("alpha\nneedle\nomega\n"));

    let output = p.walk_output(["needle"]);
    common::assert_success(&output);

    let stdout = common::normalize_stdout(&output);
    assert!(stdout.contains("utf16.txt:needle"), "stdout was {stdout:?}");
}

#[test]
fn explicit_utf16le_encoding_searches_without_bom() {
    let p = TestProject::new("integration-encoding-utf16le");
    let bytes: Vec<u8> = "alpha\nneedle\nomega\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    p.write("utf16le.txt", bytes);

    let output = p.walk_output(["--encoding", "utf-16le", "needle"]);
    common::assert_success(&output);

    let stdout = common::normalize_stdout(&output);
    assert!(
        stdout.contains("utf16le.txt:needle"),
        "stdout was {stdout:?}"
    );
}

#[test]
fn encoding_none_disables_bom_sniffing() {
    let p = TestProject::new("integration-encoding-none");
    p.write("utf16.txt", utf16le_with_bom("alpha\nneedle\nomega\n"));

    let output = p.walk_output(["--encoding", "none", "needle"]);
    common::assert_exit_code(&output, 1);
}

#[test]
fn invalid_encoding_is_rejected() {
    let p = TestProject::new("integration-encoding-invalid");
    p.write("a.txt", "needle\n");

    let output = p.walk_output(["--encoding", "not-an-encoding", "needle"]);
    common::assert_exit_code(&output, 2);
}
