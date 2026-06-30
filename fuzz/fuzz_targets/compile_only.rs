#![no_main]

use libfuzzer_sys::fuzz_target;
use sift_core::grep::{PatternCompiler, GrepMatchFlags, GrepOptions};

/// Static branches combined with random bytes to stress alternation and flag shaping.
const STATIC_BRANCHES: &[&str] = &[r"a.c", r"foo|bar", r"^line$", r"\bword\b", "(", r"[", ""];

fn compile_with_flags(patterns: &[String], opts: &GrepOptions) {
    let _ = PatternCompiler::new()
        .fixed_strings(opts.fixed_strings())
        .word_regexp(opts.word_regexp())
        .line_regexp(opts.line_regexp())
        .case_insensitive(opts.case_insensitive())
        .compile(&patterns.iter().map(String::as_str).collect::<Vec<_>>());
}

fuzz_target!(|data: &[u8]| {
    let flags = data
        .first()
        .map(|b| GrepMatchFlags::from_bits_truncate(u16::from(*b)))
        .unwrap_or_default();
    let max_results = data.get(1).map(|b| (*b as usize).min(5000));
    let opts = GrepOptions {
        flags,
        max_results,
        ..GrepOptions::default()
    };

    let rest = data.get(2..).unwrap_or_default();
    let dynamic: String = String::from_utf8_lossy(rest).chars().take(256).collect();

    for s in STATIC_BRANCHES {
        let patterns = vec![s.to_string(), dynamic.clone()];
        compile_with_flags(&patterns, &opts);
    }

    let patterns: Vec<String> = STATIC_BRANCHES.iter().map(|s| (*s).to_string()).collect();
    compile_with_flags(&patterns, &opts);

    let single = dynamic.clone();
    compile_with_flags(&[single], &opts);
});
