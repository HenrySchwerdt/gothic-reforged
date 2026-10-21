//! `modtool new`: a minimal, valid mod folder.
use anyhow::{Result, bail};
use std::path::Path;

pub fn scaffold(dir: &Path, id: &str, name: &str, data: bool, script: bool) -> Result<()> {
    if dir.join("mod.toml").exists() {
        bail!("{} already contains a mod.toml", dir.display());
    }
    std::fs::create_dir_all(dir)?;
    let mut manifest = format!(
        "[mod]\nid = \"{id}\"\nname = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"\"\nauthors = []\n\n[dependencies]\n\n[load]\nafter = []\nbefore = []\n"
    );
    if script {
        manifest.push_str("\n[script]\nentry = \"scripts/main.rhai\"\ncapabilities = [\"ui\"]\n");
        std::fs::create_dir_all(dir.join("scripts"))?;
        std::fs::write(
            dir.join("scripts/main.rhai"),
            "fn on_load() {\n    this.started = true;\n}\n\nfn on_quest_changed(e) {\n    if e.status == \"Success\" {\n        notice(`${e.quest} done`);\n    }\n}\n",
        )?;
    }
    if data {
        std::fs::create_dir_all(dir.join("patches"))?;
        std::fs::write(
            dir.join("patches/10_example.toml"),
            format!(
                "# Example: a new localized string. See docs/MODDING.md for all operations.\n[[strings.set]]\nkey = \"{}_HELLO\"\ntexts = {{ English = \"Hello\", German = \"Hallo\" }}\n",
                id.to_ascii_uppercase()
            ),
        )?;
    }
    gothic_mods_script::ScriptManifest::parse(&manifest)?;
    std::fs::write(dir.join("mod.toml"), manifest)?;
    Ok(())
}
