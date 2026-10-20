//! `AudioLibrary`: the VFS plus every audio index, loaded once from a game `Data` directory.
use crate::music::MusicLibrary;
use crate::sfx::{SfxLibrary, SoundIndex};
use crate::speech::{SpeechIndex, SpeechLanguage};
use anyhow::{Context, Result};
use gothic_formats::stringtable::{StringTable, unescape};
use gothic_formats::vfs::VirtualFileSystem;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct AudioLibrary {
    vfs: Mutex<VirtualFileSystem>,
    /// Text/subtitle language (string-table column), e.g. `German`.
    pub language: String,
    pub speech: SpeechIndex,
    pub music: MusicLibrary,
    pub sfx: SfxLibrary,
    pub sounds: SoundIndex,
    strings: Option<StringTable>,
}

impl AudioLibrary {
    /// Open `Data/*.pak` + patches and index speech (for `language`), music, sounds and sfx
    /// templates. Missing optional parts (music table, string table) degrade to empty.
    pub fn open(data_dir: &Path, language: &str) -> Result<Arc<Self>> {
        let mut vfs = VirtualFileSystem::open_data_dir(data_dir)
            .with_context(|| format!("opening {}", data_dir.display()))?;

        let speech = match SpeechLanguage::new(language) {
            Some(l) => SpeechIndex::build(l, vfs.iter().map(|(p, _)| p)),
            None => SpeechIndex::default(),
        };
        let sound_paths: Vec<String> = vfs
            .iter()
            .filter(|&(_, s)| vfs.archive_name(s).eq_ignore_ascii_case("sound.pak"))
            .map(|(p, _)| p.to_owned())
            .collect();
        let sounds = SoundIndex::build(sound_paths.iter().map(String::as_str));

        let mut music = match vfs.resolve_name("g3.mul") {
            Some(slot) => MusicLibrary::parse(&vfs.read(slot)?).unwrap_or_default(),
            None => MusicLibrary::default(),
        };
        music.retain_available(|p| vfs.resolve(p).is_some());

        let mut sfx = SfxLibrary::default();
        let tples: Vec<_> = vfs
            .iter()
            .filter(|(p, _)| p.starts_with("sfx/") && p.ends_with(".tple"))
            .map(|(p, s)| (p.to_owned(), s))
            .collect();
        for (path, slot) in tples {
            if let Ok(bytes) = vfs.read(slot) {
                sfx.add_template(&path, &bytes);
            }
        }

        let strings = vfs
            .resolve_name("stringtable.ini")
            .and_then(|slot| vfs.read(slot).ok())
            .and_then(|b| StringTable::parse(&b).ok());

        Ok(Arc::new(Self {
            vfs: Mutex::new(vfs),
            language: language.to_owned(),
            speech,
            music,
            sfx,
            sounds,
            strings,
        }))
    }

    /// Raw bytes of a VFS member.
    pub fn read(&self, path: &str) -> Result<Arc<[u8]>> {
        let mut vfs = self
            .vfs
            .lock()
            .map_err(|_| anyhow::anyhow!("vfs lock poisoned"))?;
        let slot = vfs
            .resolve(path)
            .with_context(|| format!("{path} not in the virtual file system"))?;
        Ok(vfs.read(slot)?.into())
    }

    /// Run `f` with the shared VFS (e.g. for tooling).
    pub fn with_vfs<R>(&self, f: impl FnOnce(&mut VirtualFileSystem) -> R) -> Option<R> {
        self.vfs.lock().ok().map(|mut v| f(&mut v))
    }

    /// Localized, unescaped subtitle text for a string-table key.
    pub fn subtitle(&self, key: &str) -> Option<String> {
        let table = self.strings.as_ref()?;
        let li = table.language_index(&self.language)?;
        let text = table.get_loose(key)?.texts.get(li)?;
        (!text.is_empty()).then(|| unescape(text))
    }

    pub fn strings(&self) -> Option<&StringTable> {
        self.strings.as_ref()
    }

    /// Library without any archive, for unit tests of logic that only needs the music table.
    #[cfg(test)]
    pub(crate) fn for_tests(music: MusicLibrary) -> Self {
        Self {
            vfs: Mutex::new(VirtualFileSystem::default()),
            language: "German".into(),
            speech: SpeechIndex::default(),
            music,
            sfx: SfxLibrary::default(),
            sounds: SoundIndex::default(),
            strings: None,
        }
    }
}
