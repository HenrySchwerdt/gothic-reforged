//! Writing option pages back into `UserOptions.ini`.
//!
//! The original serialises every option page into an `eCConfigFile` and writes the whole file
//! (`gCHUDMenuOptions2` apply, see `ENGINE_MAP.md` 11.1). The port edits one section at a time:
//! [`merge_section`] replaces the keys of a section and leaves every other line alone, so a
//! real user file keeps the pages this build does not edit yet.

/// UI -> host: the player applied a page. The host merges `pairs` into `section` of
/// `UserOptions.ini` with [`merge_section`] and writes the file.
#[derive(bevy::prelude::Message, Clone, Debug, PartialEq)]
pub struct OptionsWrite {
    pub section: &'static str,
    pub pairs: Vec<(String, String)>,
}

/// Replace (or add) `pairs` in `[section]` of an ini text. Existing keys keep their line
/// position and are matched case-insensitively; new keys are appended to the section, a missing
/// section is appended to the file. `strip_prefix` first drops every key of the section that
/// starts with it (case-insensitive), so a rebound key table does not keep stale entries.
pub fn merge_section(
    text: &str,
    section: &str,
    pairs: &[(String, String)],
    strip_prefix: Option<&str>,
) -> String {
    let strip = strip_prefix.map(str::to_ascii_lowercase);
    let mut lines: Vec<String> = Vec::new();
    let mut inside = false;
    let mut seen_section = false;
    let mut pending: Vec<&(String, String)> = pairs.iter().collect();
    // index after the last line of the section
    let mut section_end = 0;
    for raw in text.lines() {
        let l = raw.trim();
        if let Some(name) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            inside = name.eq_ignore_ascii_case(section);
            seen_section |= inside;
            lines.push(raw.to_string());
            if inside {
                section_end = lines.len();
            }
            continue;
        }
        if inside && let Some((k, _)) = l.split_once('=') {
            let key = k.trim().to_ascii_lowercase();
            if strip.as_deref().is_some_and(|p| key.starts_with(p)) {
                continue;
            }
            if let Some(i) = pending
                .iter()
                .position(|(pk, _)| pk.eq_ignore_ascii_case(&key))
            {
                let (pk, pv) = pending.remove(i);
                lines.push(format!("{pk}={pv}"));
                section_end = lines.len();
                continue;
            }
        }
        lines.push(raw.to_string());
        if inside && !l.is_empty() {
            section_end = lines.len();
        }
    }
    let new: Vec<String> = pending.iter().map(|(k, v)| format!("{k}={v}")).collect();
    if seen_section {
        lines.splice(section_end..section_end, new);
    } else {
        lines.push(format!("[{section}]"));
        lines.extend(new);
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// The `key=value` pairs of one section, in file order (keys keep their spelling).
pub fn section_pairs(text: &str, section: &str) -> Vec<(String, String)> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(name) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            inside = name.eq_ignore_ascii_case(section);
        } else if inside && let Some((k, v)) = l.split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(k: &str, v: &str) -> (String, String) {
        (k.to_string(), v.to_string())
    }

    #[test]
    fn existing_keys_are_replaced_in_place() {
        let t = "[A]\nX=1\nY=2\n[B]\nZ=3\n";
        let out = merge_section(t, "a", &[kv("y", "9")], None);
        assert_eq!(out, "[A]\nX=1\ny=9\n[B]\nZ=3\n");
    }

    #[test]
    fn new_keys_land_at_the_end_of_their_section() {
        let t = "[A]\nX=1\n\n[B]\nZ=3\n";
        let out = merge_section(t, "A", &[kv("W", "4")], None);
        assert_eq!(out, "[A]\nX=1\nW=4\n\n[B]\nZ=3\n");
    }

    #[test]
    fn a_missing_section_is_appended() {
        let out = merge_section("[A]\nX=1\n", "C", &[kv("K", "v")], None);
        assert_eq!(out, "[A]\nX=1\n[C]\nK=v\n");
    }

    #[test]
    fn prefix_stripping_removes_stale_bindings() {
        let t = "[Options.Controls]\nControls=Custom\nSessionKey.Up.Key2.Type=0\nSessionKey.Up.Key2.Offset=5\n";
        let out = merge_section(
            t,
            "Options.Controls",
            &[kv("SessionKey.Up.Key1.Type", "0")],
            Some("SessionKey."),
        );
        assert!(out.contains("Controls=Custom"));
        assert!(!out.contains("Key2"));
        assert!(out.contains("SessionKey.Up.Key1.Type=0"));
    }

    #[test]
    fn section_pairs_reads_one_section() {
        let t = "[A]\nX = 1\n[B]\nY=2\n";
        assert_eq!(section_pairs(t, "b"), vec![kv("Y", "2")]);
    }
}
