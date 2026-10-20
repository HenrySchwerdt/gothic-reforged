//! Lenient record parser shared by the plain-text Gothic 3 game-data files
//! (`.quest`, `.info`): a `[Header]` line followed by `Key=Value` lines (CRLF).
//!
//! The installed files are Latin-1/ASCII. A line without `=` is a key with an
//! empty value (the game writes `Parent` instead of `Parent=`). Real files also
//! contain a small number of malformed records; those are parsed and reported as
//! [`Anomaly`] values instead of failing, so surveys can count them exactly.
use anyhow::{Result, ensure};
use serde::Serialize;

pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_LINES: usize = 4096;
const MAX_LINE: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Anomaly {
    pub kind: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Record {
    /// Header text between the brackets, e.g. `Quest` or `Info`.
    pub header: String,
    /// Fields in file order. Duplicate keys are kept (first wins on lookup).
    pub fields: Vec<(String, String)>,
    pub anomalies: Vec<Anomaly>,
}

impl Record {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Parse a record. `expected_header` is e.g. `"Quest"`.
pub fn parse_record(bytes: &[u8], expected_header: &str) -> Result<Record> {
    ensure!(
        bytes.len() <= MAX_RECORD_BYTES,
        "record exceeds {MAX_RECORD_BYTES} bytes"
    );
    // Latin-1: every byte maps to the same code point; never fails.
    let text: String = bytes.iter().map(|&b| char::from(b)).collect();
    let mut record = Record::default();
    let mut lines = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
    let mut count = 0usize;
    // Header: first non-empty line.
    let mut header_seen = false;
    let mut seen_keys: Vec<&str> = Vec::new();
    for line in &mut lines {
        count += 1;
        ensure!(count <= MAX_LINES, "record line count exceeds limit");
        ensure!(line.len() <= MAX_LINE, "record line exceeds limit");
        if line.is_empty() {
            continue;
        }
        if !header_seen {
            header_seen = true;
            let want = format!("[{expected_header}]");
            if line == want {
                record.header = expected_header.to_string();
            } else if let Some(prefix) = line.strip_suffix(&want) {
                record.header = expected_header.to_string();
                record.anomalies.push(Anomaly {
                    kind: "header_prefix_junk",
                    detail: format!("{} byte(s) before header", prefix.len()),
                });
            } else {
                ensure!(false, "missing [{expected_header}] header");
            }
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            record.anomalies.push(Anomaly {
                kind: "extra_section",
                detail: line.to_string(),
            });
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        if key.contains(';') || key.contains(' ') || key.is_empty() {
            record.anomalies.push(Anomaly {
                kind: "malformed_key",
                detail: key.to_string(),
            });
        }
        if seen_keys.contains(&key) {
            record.anomalies.push(Anomaly {
                kind: "duplicate_key",
                detail: key.to_string(),
            });
        } else {
            seen_keys.push(key);
        }
        record.fields.push((key.to_string(), value.to_string()));
    }
    ensure!(header_seen, "empty record");
    Ok(record)
}

/// Split a `;`-separated list. A single trailing empty element (from a trailing
/// `;`) is dropped; interior empty elements are kept because several parallel
/// arrays rely on them as positional placeholders.
pub fn split_list(value: &str) -> Vec<&str> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut parts: Vec<&str> = value.split(';').collect();
    if parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_without_equals_and_junk_header() {
        let r = parse_record(b"y[Info]\r\nName=A\r\nParent\r\nName=B\r\n", "Info").unwrap();
        assert_eq!(r.get("Name"), Some("A"));
        assert_eq!(r.get("Parent"), Some(""));
        let kinds: Vec<_> = r.anomalies.iter().map(|a| a.kind).collect();
        assert_eq!(kinds, ["header_prefix_junk", "duplicate_key"]);
    }

    #[test]
    fn rejects_missing_header_and_oversize() {
        assert!(parse_record(b"Name=A", "Info").is_err());
        assert!(parse_record(b"", "Info").is_err());
        assert!(parse_record(&vec![b'a'; MAX_RECORD_BYTES + 1], "Info").is_err());
    }

    #[test]
    fn split_list_keeps_placeholders() {
        assert_eq!(split_list("a;;b;"), ["a", "", "b"]);
        assert_eq!(split_list(";"), [""]);
        assert!(split_list("").is_empty());
    }

    #[test]
    fn never_panics_on_arbitrary_bytes() {
        for n in 0..=255u8 {
            let _ = parse_record(&[b'[', n, b']', b'\r', b'\n', n, b'='], "Info");
        }
    }
}
