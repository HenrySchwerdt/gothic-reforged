//! The example mods round-trip through validate -> pack -> verify -> install -> validate;
//! plus signing, tampering, upgrades, unsafe packages, ordering and conflicts.
use gothic_modtool::package::{signing_key, unhex};
use gothic_modtool::{
    InstallOptions, ModList, ModStatus, Package, SignatureStatus, install, keygen, pack,
    static_conflicts, uninstall, validate_dir,
};
use std::io::Write;
use std::path::{Path, PathBuf};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/examples")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gr-modtool-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_mod(dir: &Path, id: &str, version: &str, extra: &str, patch: &str) {
    std::fs::create_dir_all(dir.join("patches")).unwrap();
    std::fs::write(
        dir.join("mod.toml"),
        format!("[mod]\nid = \"{id}\"\nversion = \"{version}\"\n{extra}"),
    )
    .unwrap();
    std::fs::write(dir.join("patches/10.toml"), patch).unwrap();
}

#[test]
fn example_mods_round_trip() {
    let mods = tmp("roundtrip");
    for name in ["forest_hermit", "chicken_rain", "orc_rhymes"] {
        let src = examples().join(name);
        let r = validate_dir(&src);
        assert!(r.ok(), "{}", r.to_text());
        let (manifest, bytes) = pack(&src, None).unwrap();
        assert_eq!(manifest.id, name);
        // Deterministic packaging.
        assert_eq!(pack(&src, None).unwrap().1, bytes);
        let pkg = Package::open(&bytes).unwrap();
        assert_eq!(pkg.signature_status(), SignatureStatus::Unsigned);
        let report = install(&bytes, &mods, &InstallOptions::default()).unwrap();
        assert_eq!(report.id, name);
        // Installed copy is byte-identical and still validates.
        let installed = mods.join(name);
        assert!(validate_dir(&installed).ok());
        for f in &manifest.files {
            assert_eq!(
                std::fs::read(installed.join(&f.path)).unwrap(),
                std::fs::read(src.join(&f.path)).unwrap()
            );
        }
    }
    let list = ModList::load(&mods).unwrap();
    let entries = list.entries();
    assert_eq!(entries.len(), 3);
    assert!(entries.iter().all(|e| e.status == ModStatus::Active));
    assert!(
        entries
            .iter()
            .any(|e| e.id == "chicken_rain" && e.has_script)
    );
    assert!(
        entries
            .iter()
            .any(|e| e.id == "forest_hermit" && e.patch_files == 4)
    );
    assert!(mods.join("modlist.toml").is_file());
    uninstall(&mods, "orc_rhymes").unwrap();
    assert_eq!(ModList::load(&mods).unwrap().entries().len(), 2);
}

#[test]
fn signatures_and_tampering() {
    let (secret, public) = keygen().unwrap();
    let key = signing_key(&secret).unwrap();
    let src = examples().join("forest_hermit");
    let (_, signed) = pack(&src, Some(&key)).unwrap();
    let pkg = Package::open(&signed).unwrap();
    assert_eq!(
        pkg.signature_status(),
        SignatureStatus::Valid {
            signer: public.clone()
        }
    );
    let mods = tmp("signed");
    let trusted = InstallOptions {
        trusted_keys: vec![public.clone()],
        ..Default::default()
    };
    install(&signed, &mods, &trusted).unwrap();

    // Untrusted signer and unsigned packages are refused when a trust list is given.
    let (_, other_public) = keygen().unwrap();
    let wrong = InstallOptions {
        trusted_keys: vec![other_public],
        force: true,
        ..Default::default()
    };
    assert!(install(&signed, &mods, &wrong).is_err());
    let (_, unsigned) = pack(&src, None).unwrap();
    assert!(
        install(
            &unsigned,
            &mods,
            &InstallOptions {
                force: true,
                ..trusted.clone()
            }
        )
        .is_err()
    );

    // Rewrite one file inside the zip: the hash check catches it.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&signed)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut b).unwrap();
        if f.name() == "patches/10_items.toml" {
            // Same size, different content.
            b[0] ^= 0x20;
        }
        out.start_file(f.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(&b).unwrap();
    }
    let tampered = out.finish().unwrap().into_inner();
    let err = Package::open(&tampered).unwrap_err().to_string();
    assert!(err.contains("hash mismatch"), "{err}");
    assert_eq!(unhex(&public).unwrap().len(), 32);
}

#[test]
fn upgrades_downgrades_and_dependencies() {
    let work = tmp("versions");
    let mods = work.join("mods");
    let add = |id: &str, version: &str, extra: &str| {
        let d = work.join(format!("{id}-{version}"));
        write_mod(
            &d,
            id,
            version,
            extra,
            &format!(
                "[[strings.set]]\nkey = \"{}\"\ntexts = {{ English = \"{version}\" }}\n",
                id.to_uppercase()
            ),
        );
        pack(&d, None).unwrap().1
    };
    let base1 = add("base", "1.0.0", "");
    let base2 = add("base", "1.2.0", "");
    let addon = add("addon", "1.0.0", "[dependencies]\nbase = \">=1.1\"\n");
    install(&addon, &mods, &InstallOptions::default()).unwrap();
    // Dependency missing: installed, but reported and not active.
    let e = ModList::load(&mods).unwrap().entries();
    assert_eq!(e[0].status, ModStatus::Problem, "{e:?}");
    install(&base1, &mods, &InstallOptions::default()).unwrap();
    let e = ModList::load(&mods).unwrap().entries();
    let addon_e = e.iter().find(|x| x.id == "addon").unwrap();
    assert_eq!(addon_e.status, ModStatus::Problem);
    assert!(
        addon_e.problems[0].contains(">=1.1"),
        "{:?}",
        addon_e.problems
    );
    let r = install(&base2, &mods, &InstallOptions::default()).unwrap();
    assert_eq!(r.replaced.as_deref(), Some("1.0.0"));
    assert!(
        install(&base1, &mods, &InstallOptions::default()).is_err(),
        "no downgrade"
    );
    install(
        &base1,
        &mods,
        &InstallOptions {
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    install(&base2, &mods, &InstallOptions::default()).unwrap();
    // Dependency forces base before addon even though addon is first in the user list.
    let list = ModList::load(&mods).unwrap();
    assert_eq!(list.load_order().0, ["base", "addon"]);
}

#[test]
fn user_order_breaks_ties_and_conflicts_follow_it() {
    let work = tmp("order");
    let mods = work.join("mods");
    for (id, value) in [("aaa", 10), ("zzz", 99)] {
        let d = work.join(id);
        write_mod(
            &d,
            id,
            "1.0.0",
            "",
            &format!(
                "[[items.patch]]\nname = \"It_Herb\"\nset = {{ value = {value} }}\n[[strings.set]]\nkey = \"SHARED\"\ntexts = {{ English = \"{id}\" }}\n"
            ),
        );
        install(
            &pack(&d, None).unwrap().1,
            &mods,
            &InstallOptions::default(),
        )
        .unwrap();
    }
    let mut list = ModList::load(&mods).unwrap();
    assert_eq!(list.load_order().0, ["aaa", "zzz"]);
    let c = static_conflicts(&list.active_mods());
    assert_eq!(c.len(), 2);
    assert!(c.iter().all(|x| x.winner == "zzz" && x.earlier == "aaa"));
    assert_eq!(
        c[0].to_string(),
        "item It_Herb field value: zzz overrides aaa"
    );
    // The player moves zzz to the top: now aaa wins.
    assert!(list.move_to("zzz", 0));
    list.save().unwrap();
    let list = ModList::load(&mods).unwrap();
    assert_eq!(list.load_order().0, ["zzz", "aaa"]);
    assert!(
        static_conflicts(&list.active_mods())
            .iter()
            .all(|x| x.winner == "aaa")
    );
    // Disabled mods drop out.
    let mut list = ModList::load(&mods).unwrap();
    list.set_enabled("aaa", false);
    list.save().unwrap();
    let list = ModList::load(&mods).unwrap();
    assert_eq!(list.load_order().0, ["zzz"]);
    assert!(static_conflicts(&list.active_mods()).is_empty());
    assert_eq!(list.entries()[1].status, ModStatus::Disabled);
}

#[test]
fn bad_mods_and_unsafe_packages_are_rejected() {
    let work = tmp("bad");
    // Unknown field and a disallowed file type.
    let d = work.join("bad");
    write_mod(
        &d,
        "bad",
        "1.0.0",
        "",
        "[[items.add]]\nname = \"It_X\"\nvalu = 3\n",
    );
    std::fs::write(d.join("model.xcmsh"), b"GENOMFLE").unwrap();
    let r = validate_dir(&d);
    assert!(!r.ok());
    assert!(
        r.errors.iter().any(|e| e.contains("valu")),
        "{:?}",
        r.errors
    );
    assert!(
        r.errors.iter().any(|e| e.contains("model.xcmsh")),
        "{:?}",
        r.errors
    );
    assert!(pack(&d, None).is_err());
    // Broken script.
    let s = work.join("script");
    std::fs::create_dir_all(&s).unwrap();
    std::fs::write(
        s.join("mod.toml"),
        "[mod]\nid = \"script\"\nversion = \"1.0.0\"\n[script]\ncapabilities = [\"ui\"]\n",
    )
    .unwrap();
    std::fs::write(s.join("main.rhai"), "fn on_tick(e) { notice( }").unwrap();
    assert!(!validate_dir(&s).ok());
    // Zip-slip and unlisted files.
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    out.start_file("grmod.toml", o).unwrap();
    out.write_all(b"format = 1\nid = \"evil\"\nversion = \"1.0.0\"\nfiles = []\n")
        .unwrap();
    out.start_file("../evil.toml", o).unwrap();
    out.write_all(b"x").unwrap();
    let evil = out.finish().unwrap().into_inner();
    assert!(
        Package::open(&evil)
            .unwrap_err()
            .to_string()
            .contains("unsafe path")
    );
    assert!(Package::open(b"not a zip").is_err());
}
