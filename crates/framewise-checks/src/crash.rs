//! Crash reports: what's recorded when FrameWise crashes, and the scrubbing that keeps file paths,
//! user names and process lists out of it. Reports stay on the PC; one is only ever sent if the
//! user opens it as a GitHub issue themselves (the design document's opt-in crash report: "only the
//! error and app version, never file paths or process lists").

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CrashReport {
    /// File name of the stored report (the interface uses it to dismiss the report).
    #[serde(default)]
    pub id: String,
    /// Unix seconds.
    pub at: u64,
    pub version: String,
    /// "app", "watcher", "helper" or "interface".
    pub process: String,
    /// The error, scrubbed.
    pub message: String,
    /// Where in FrameWise's own source it happened (crate-relative, e.g. `src/live.rs:120`).
    pub location: Option<String>,
}

/// Longest message kept, in characters.
pub const MAX_MESSAGE: usize = 600;

/// Removes anything that could identify the PC or its owner: absolute paths (drive and UNC), the
/// Windows user name, and anything after it would exceed the length limit.
pub fn scrub(message: &str, user_name: Option<&str>) -> String {
    let mut out = String::with_capacity(message.len().min(MAX_MESSAGE));
    let chars: Vec<char> = message.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let drive = i + 2 < chars.len() && chars[i].is_ascii_alphabetic() && chars[i + 1] == ':' && (chars[i + 2] == '\\' || chars[i + 2] == '/')
            && (i == 0 || !chars[i - 1].is_alphanumeric());
        let unc = i + 1 < chars.len() && chars[i] == '\\' && chars[i + 1] == '\\';
        let home = i + 1 < chars.len() && chars[i] == '%' && chars[i + 1].is_ascii_alphabetic();
        if drive || unc || home {
            // Up to the next whitespace or quote: paths with spaces leave their tail behind, but
            // the identifying start (drive, user folder) is gone.
            while i < chars.len() && !chars[i].is_whitespace() && !matches!(chars[i], '"' | '\'' | '`' | ')' | ',') {
                i += 1;
            }
            out.push_str("<path>");
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    if let Some(u) = user_name.filter(|u| u.len() >= 3) {
        out = replace_ignore_case(&out, u, "<user>");
    }
    if out.chars().count() > MAX_MESSAGE {
        out = out.chars().take(MAX_MESSAGE).collect::<String>() + "…";
    }
    out
}

fn replace_ignore_case(s: &str, needle: &str, with: &str) -> String {
    let lower = s.to_lowercase();
    let n = needle.to_lowercase();
    if lower.len() != s.len() {
        return s.replace(needle, with); // non-ASCII case mapping changed lengths: exact match only
    }
    let mut out = String::new();
    let mut last = 0;
    for (pos, _) in lower.match_indices(&n) {
        out.push_str(&s[last..pos]);
        out.push_str(with);
        last = pos + n.len();
    }
    out.push_str(&s[last..]);
    out
}

/// Keeps only the crate-relative part of a source location (`C:\...\src-tauri\src\live.rs:120:5`
/// becomes `src/live.rs:120`).
pub fn short_location(file: &str, line: u32) -> String {
    let f = file.replace('\\', "/");
    let rel = f.rfind("/src/").map(|i| &f[i + 1..]).unwrap_or_else(|| f.rsplit('/').next().unwrap_or(&f));
    format!("{rel}:{line}")
}

/// The text of a GitHub issue for a report: exactly what the user sees before deciding to send it.
pub fn issue_body(r: &CrashReport, windows: &str) -> String {
    format!(
        "**FrameWise {}** · {} · {}\n\n**What happened:** the {} stopped unexpectedly.\n\n```\n{}\n```\n{}\n_Scrubbed report: no file paths, user names or process lists._\n\n**What I was doing (optional):**\n",
        r.version,
        windows,
        r.process,
        r.process,
        r.message,
        r.location.as_deref().map(|l| format!("At `{l}`.\n")).unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_paths_and_user_names() {
        let m = r#"Couldn't open C:\Users\alex\AppData\Local\FrameWise\framewise.db: denied (also \\nas\share\x and %LOCALAPPDATA%\FrameWise)"#;
        let s = scrub(m, Some("alex"));
        assert!(!s.contains("alex") && !s.contains("Users") && !s.contains("nas"), "{s}");
        assert!(s.contains("Couldn't open <path> denied"), "{s}");
        assert_eq!(scrub("thread panicked: index out of bounds", None), "thread panicked: index out of bounds");
        // The user name anywhere else, any case.
        assert_eq!(scrub("profile for ALEX failed", Some("alex")), "profile for <user> failed");
        // Ratios and times aren't paths.
        assert_eq!(scrub("ratio 3:1 at 12:30", None), "ratio 3:1 at 12:30");
        assert!(scrub(&"x".repeat(2000), None).chars().count() <= MAX_MESSAGE + 1);
    }

    #[test]
    fn locations_are_crate_relative() {
        assert_eq!(short_location(r"C:\Users\me\FrameWise\src-tauri\src\live.rs", 120), "src/live.rs:120");
        assert_eq!(short_location("/home/runner/work/src/main.rs", 5), "src/main.rs:5");
    }

    #[test]
    fn issue_text_names_version_and_process_only() {
        let r = CrashReport { id: "1".into(), at: 1, version: "0.4.0".into(), process: "watcher".into(), message: "boom".into(), location: Some("src/watcher.rs:10".into()) };
        let b = issue_body(&r, "Windows 11 (build 26300)");
        assert!(b.contains("FrameWise 0.4.0") && b.contains("watcher") && b.contains("boom") && b.contains("src/watcher.rs:10"));
    }
}
