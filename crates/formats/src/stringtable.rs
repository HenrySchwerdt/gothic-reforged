//! Localized string table (`Strings.pak/stringtable.ini`) and per-world tables
//! (`G3_World_01_StringTable.ini`).
//!
//! Layout (observed, fully decoded): UTF-16LE with BOM `FF FE`, CRLF lines, three
//! sections: `[LocAdmin_Languages]` (`Languages=English;Italian;...`,
//! `CurrentLanguage=`, `CurrentRevision=`), `[LocAdmin_Strings]` with
//! `KEY=text0;;text1;;...;;textN;` (one text per language, in `Languages` order,
//! the last one terminated by a single `;`), and an empty `[LocAdmin_Revisions]`.
//! Literal `\n` two-character sequences inside texts are the game's line breaks
//! and are preserved raw (see [`unescape`]).
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_000_000;
const MAX_LANGUAGES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StringEntry {
    pub key: String,
    /// One text per language in [`StringTable::languages`] order.
    pub texts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StringTable {
    pub languages: Vec<String>,
    pub current_language: String,
    pub current_revision: String,
    pub entries: Vec<StringEntry>,
    /// Keys whose text count differed from the language count (padded/truncated).
    pub irregular_keys: Vec<String>,
    /// Keys defined more than once (first definition wins).
    pub duplicate_keys: Vec<String>,
    #[serde(skip)]
    index: HashMap<String, usize>,
    #[serde(skip)]
    index_lower: HashMap<String, usize>,
}

impl StringTable {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_BYTES, "string table exceeds ceiling");
        ensure!(
            bytes.len() >= 2 && bytes[..2] == [0xFF, 0xFE],
            "missing UTF-16LE BOM"
        );
        ensure!(bytes.len().is_multiple_of(2), "odd UTF-16 byte length");
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let Ok(text) = String::from_utf16(&units) else {
            bail!("invalid UTF-16 (unpaired surrogate)");
        };
        let mut table = Self::default();
        #[derive(PartialEq)]
        enum Section {
            None,
            Languages,
            Strings,
            Other,
        }
        let mut section = Section::None;
        for line in text.split("\r\n") {
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = match line {
                    "[LocAdmin_Languages]" => Section::Languages,
                    "[LocAdmin_Strings]" => Section::Strings,
                    _ => Section::Other,
                };
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                bail!(
                    "line without '=' in table: {:?}",
                    line.chars().take(40).collect::<String>()
                );
            };
            match section {
                Section::Languages => match key {
                    "Languages" => {
                        table.languages = value.split(';').map(str::to_string).collect();
                        ensure!(
                            !table.languages.is_empty() && table.languages.len() <= MAX_LANGUAGES,
                            "bad language list"
                        );
                    }
                    "CurrentLanguage" => table.current_language = value.to_string(),
                    "CurrentRevision" => table.current_revision = value.to_string(),
                    _ => {}
                },
                Section::Strings => {
                    ensure!(!table.languages.is_empty(), "strings before language list");
                    ensure!(table.entries.len() < MAX_ENTRIES, "too many entries");
                    let value = value.strip_suffix(';').unwrap_or(value);
                    let mut texts: Vec<String> = value.split(";;").map(str::to_string).collect();
                    if texts.len() != table.languages.len() {
                        table.irregular_keys.push(key.to_string());
                        texts.resize(table.languages.len(), String::new());
                    }
                    if table.index.contains_key(key) {
                        table.duplicate_keys.push(key.to_string());
                        continue;
                    }
                    let i = table.entries.len();
                    table.index.insert(key.to_string(), i);
                    table
                        .index_lower
                        .entry(key.to_ascii_lowercase())
                        .or_insert(i);
                    table.entries.push(StringEntry {
                        key: key.to_string(),
                        texts,
                    });
                }
                Section::None | Section::Other => {}
            }
        }
        ensure!(
            !table.languages.is_empty(),
            "no [LocAdmin_Languages] section"
        );
        Ok(table)
    }

    /// Rebuild the key indexes (they are not serialized); call after deserializing.
    pub fn rebuild_index(&mut self) {
        self.index.clear();
        self.index_lower.clear();
        for (i, e) in self.entries.iter().enumerate() {
            self.index.entry(e.key.clone()).or_insert(i);
            self.index_lower.entry(e.key.to_ascii_lowercase()).or_insert(i);
        }
    }

    pub fn language_index(&self, language: &str) -> Option<usize> {
        self.languages
            .iter()
            .position(|l| l.eq_ignore_ascii_case(language))
    }

    pub fn get(&self, key: &str) -> Option<&StringEntry> {
        self.index.get(key).map(|&i| &self.entries[i])
    }

    /// Exact lookup first, then ASCII-case-insensitive.
    pub fn get_loose(&self, key: &str) -> Option<&StringEntry> {
        self.get(key).or_else(|| {
            self.index_lower
                .get(&key.to_ascii_lowercase())
                .map(|&i| &self.entries[i])
        })
    }

    /// Text for `key` in `language`, falling back to nothing (no cross-language guess).
    pub fn text(&self, key: &str, language: &str) -> Option<&str> {
        let li = self.language_index(language)?;
        self.get(key).map(|e| e.texts[li].as_str())
    }
}

/// Convert the literal two-character `\n` escapes to real line breaks.
pub fn unescape(s: &str) -> String {
    s.replace("\\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        let mut b = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        b
    }

    const SAMPLE: &str = "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\nA=Hello\\nWorld;;Hallo Welt;\r\nB=x;;y z;\r\nShort=only;\r\n[LocAdmin_Revisions]\r\n";

    #[test]
    fn parses_languages_and_entries() {
        let t = StringTable::parse(&utf16(SAMPLE)).unwrap();
        assert_eq!(t.languages, ["English", "German"]);
        assert_eq!(t.text("A", "german"), Some("Hallo Welt"));
        assert_eq!(unescape(t.text("A", "English").unwrap()), "Hello\nWorld");
        assert_eq!(t.irregular_keys, ["Short"]);
        assert_eq!(t.get_loose("b").unwrap().key, "B");
        assert!(t.get("missing").is_none());
    }

    #[test]
    fn reports_duplicates_and_rejects_garbage() {
        let s = SAMPLE.replace("B=x;;y z;", "A=x;;y;");
        let t = StringTable::parse(&utf16(&s)).unwrap();
        assert_eq!(t.duplicate_keys, ["A"]);
        assert!(StringTable::parse(b"no bom").is_err());
        assert!(StringTable::parse(&[0xFF, 0xFE, 0x41]).is_err());
        assert!(StringTable::parse(&[0xFF, 0xFE, 0x00, 0xD8]).is_err());
        assert!(StringTable::parse(&utf16("[LocAdmin_Strings]\r\nA=b;\r\n")).is_err());
    }

    #[test]
    fn truncation_never_panics() {
        let b = utf16(SAMPLE);
        for n in 0..b.len() {
            let _ = StringTable::parse(&b[..n]);
        }
    }
}
