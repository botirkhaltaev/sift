//! Tab-separated machine-readable lines: `profile\tkey\tvalue`.

pub fn print_profile(key: &str, value: &str) {
    println!("profile\t{key}\t{value}");
}

#[cfg(target_os = "linux")]
pub fn resident_set_bytes() -> Option<usize> {
    let path = format!("/proc/{}/status", std::process::id());
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: usize = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

#[cfg(target_os = "macos")]
pub fn resident_set_bytes() -> Option<usize> {
    use std::process::Command;
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout);
    let kb: usize = s.trim().parse().ok()?;
    Some(kb * 1024)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub const fn resident_set_bytes() -> Option<usize> {
    None
}

pub fn rss_enabled() -> bool {
    std::env::var("SIFT_PROFILE_RSS")
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}
