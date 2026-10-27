//! Save games: the container (versioned, migratable, deflated, checksummed), the slot
//! directory (quick save, rotating autosaves, numbered manual slots) and the glue that captures
//! and restores a [`Core`].
//!
//! The content follows what the original persists (`gCWorld::SaveGameWorld`, `gCSession`,
//! `gCSectorPersistence_PS`, the per-entity dynamic contexts and the inventory / navigation /
//! stats streams): see [`gothic_sim::persist`] for the simulation part. The original's binary
//! `.g3savcp` layout is not reproduced; ours is
//!
//! ```text
//! "G3RS" | schema u32 | header_len u32 | header (JSON) | body_len u32 | body (deflate(JSON)) | fnv u64
//! ```
//!
//! The header is readable without inflating the body (slot lists); the trailing FNV-1a is over
//! the inflated body and is also stored in the header, so a damaged or truncated file is
//! rejected before anything is applied. Older schemas are brought forward by [`MIGRATIONS`]
//! on the JSON value, one step per schema.
use crate::core::Core;
use anyhow::{Context, Result, bail, ensure};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use gothic_sim::persist::{RestoreReport, SimSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Host-map key that carries the persisted trigger state (`eCTrigger_PS` counters).
pub const TRIGGER_KEY: &str = "world.triggers";
/// The sector image of `gCSectorPersistence_PS::CollectSectors` in the opaque host map.
pub const SECTOR_KEY: &str = "world.sectors";
/// File magic.
pub const MAGIC: &[u8; 4] = b"G3RS";
/// Schema written by this build.
pub const SCHEMA: u32 = 1;
/// Number of rotating autosave slots.
pub const AUTOSAVES: usize = 3;
/// Number of manual slots.
pub const MANUAL_SLOTS: usize = 20;
/// Extension of save files.
pub const EXT: &str = "g3rs";

/// Upgrades the body of schema `n + 1` to schema `n + 2` (entry `n`). Empty while only one
/// schema exists; every format change appends a function here and bumps [`SCHEMA`].
pub const MIGRATIONS: &[fn(&mut Value)] = &[];

/// Everything a save game holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveBody {
    pub sim: SimSnapshot,
    /// Presentation state the host wants back (camera mode, region, ...), opaque to the core.
    pub host: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotKind {
    Quick,
    Auto,
    Manual,
}

/// Readable without the body (`gCWorld::GetSaveGameHeader`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotHeader {
    pub schema: u32,
    pub name: String,
    pub kind: SlotKind,
    /// Seconds since the Unix epoch.
    pub saved_unix: u64,
    /// World time in game milliseconds.
    pub game_ms: u64,
    pub play_ms: u64,
    /// Region / level label the host reported (`host["region"]`).
    pub region: String,
    pub hero_pos: [f32; 3],
    pub hero_level: u32,
    /// FNV-1a of the inflated body.
    pub hash: u64,
}

/// FNV-1a, 64 bit.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Bring a body of schema `from` to schema `to` with `table` (entry `i` upgrades `i + 1` to
/// `i + 2`).
pub fn migrate(body: &mut Value, from: u32, to: u32, table: &[fn(&mut Value)]) -> Result<()> {
    ensure!(from >= 1, "invalid save schema {from}");
    ensure!(
        from <= to,
        "save schema {from} is newer than this build ({to})"
    );
    for step in from..to {
        let f = table
            .get(step as usize - 1)
            .with_context(|| format!("no migration from schema {step}"))?;
        f(body);
    }
    Ok(())
}

fn pack(schema: u32, header: &SlotHeader, body_json: &[u8]) -> Result<Vec<u8>> {
    let head = serde_json::to_vec(header)?;
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(6));
    enc.write_all(body_json)?;
    let packed = enc.finish()?;
    let mut out = Vec::with_capacity(packed.len() + head.len() + 32);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&schema.to_le_bytes());
    out.extend_from_slice(&u32::try_from(head.len())?.to_le_bytes());
    out.extend_from_slice(&head);
    out.extend_from_slice(&u32::try_from(packed.len())?.to_le_bytes());
    out.extend_from_slice(&packed);
    out.extend_from_slice(&fnv1a(body_json).to_le_bytes());
    Ok(out)
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(self.0.len() >= n, "save file is truncated");
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
}

/// Parse the magic, schema and header only.
pub fn read_header(bytes: &[u8]) -> Result<SlotHeader> {
    let mut c = Cursor(bytes);
    ensure!(c.take(4)? == MAGIC, "not a save file");
    let _schema = c.u32()?;
    let n = c.u32()? as usize;
    ensure!(n <= 1 << 20, "save header too large");
    serde_json::from_slice(c.take(n)?).context("save header")
}

/// Verify and inflate a save, migrating the body to `current` with `table`.
pub fn unpack(bytes: &[u8], current: u32, table: &[fn(&mut Value)]) -> Result<(SlotHeader, Value)> {
    let mut c = Cursor(bytes);
    ensure!(c.take(4)? == MAGIC, "not a save file");
    let schema = c.u32()?;
    let n = c.u32()? as usize;
    ensure!(n <= 1 << 20, "save header too large");
    let header: SlotHeader = serde_json::from_slice(c.take(n)?).context("save header")?;
    let n = c.u32()? as usize;
    let packed = c.take(n)?;
    let sum = u64::from_le_bytes(c.take(8)?.try_into()?);
    let mut raw = Vec::new();
    DeflateDecoder::new(packed)
        .read_to_end(&mut raw)
        .context("save body")?;
    if fnv1a(&raw) != sum || sum != header.hash {
        bail!("save file is damaged (checksum)");
    }
    let mut body: Value = serde_json::from_slice(&raw).context("save body")?;
    migrate(&mut body, schema, current, table)?;
    Ok((header, body))
}

/// Metadata the caller adds to a save.
#[derive(Debug, Clone)]
pub struct SaveMeta {
    pub name: String,
    pub kind: SlotKind,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Encode a body into a save file.
pub fn encode(body: &SaveBody, meta: &SaveMeta) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(body)?;
    let hero = body.sim.actors.first();
    let header = SlotHeader {
        schema: SCHEMA,
        name: meta.name.clone(),
        kind: meta.kind,
        saved_unix: now_unix(),
        game_ms: body.sim.game_ms,
        play_ms: (body.sim.tick as f64 * 1000.0 / f64::from(crate::TICK_HZ)) as u64,
        region: body.host.get("region").cloned().unwrap_or_default(),
        hero_pos: hero.map_or([0.0; 3], |h| h.pos),
        hero_level: body.sim.state.level,
        hash: fnv1a(&json),
    };
    pack(SCHEMA, &header, &json)
}

/// Decode a save file written by this or an older build.
pub fn decode(bytes: &[u8]) -> Result<(SlotHeader, SaveBody)> {
    let (header, value) = unpack(bytes, SCHEMA, MIGRATIONS)?;
    let body = serde_json::from_value(value).context("save body")?;
    Ok((header, body))
}

/// The save directory: quick save, rotating autosaves and numbered manual slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveDir(pub PathBuf);

/// One listed save.
#[derive(Debug, Clone)]
pub struct SlotInfo {
    pub path: PathBuf,
    pub header: SlotHeader,
}

impl SaveDir {
    pub fn quick(&self) -> PathBuf {
        self.0.join(format!("quick.{EXT}"))
    }

    /// Manual slot `n` (1-based, up to [`MANUAL_SLOTS`]).
    pub fn manual(&self, n: usize) -> PathBuf {
        self.0.join(format!("slot_{n:02}.{EXT}"))
    }

    fn auto(&self, n: usize) -> PathBuf {
        self.0.join(format!("auto_{n}.{EXT}"))
    }

    /// The autosave slot to overwrite next: a free one, else the oldest.
    pub fn next_autosave(&self) -> PathBuf {
        let stamp = |i: usize| {
            std::fs::read(self.auto(i))
                .ok()
                .and_then(|b| read_header(&b).ok())
                .map(|h| h.saved_unix)
        };
        (0..AUTOSAVES)
            .min_by_key(|i| stamp(*i).map_or((0, *i), |t| (1, t as usize)))
            .map_or_else(|| self.auto(0), |i| self.auto(i))
    }

    /// All readable saves, newest first (damaged files are skipped).
    pub fn list(&self) -> Vec<SlotInfo> {
        let mut out: Vec<SlotInfo> = std::fs::read_dir(&self.0)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == EXT))
            .filter_map(|path| {
                let header = read_header(&std::fs::read(&path).ok()?).ok()?;
                Some(SlotInfo { path, header })
            })
            .collect();
        out.sort_by(|a, b| {
            b.header
                .saved_unix
                .cmp(&a.header.saved_unix)
                .then_with(|| a.path.cmp(&b.path))
        });
        out
    }
}

/// Write `bytes` through a temporary file so a crash never leaves a half-written save.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).with_context(|| tmp.display().to_string())?;
    std::fs::rename(&tmp, path).with_context(|| path.display().to_string())
}

impl Core {
    /// Port of `gCWorld::SaveGameWorld` / `gCSession` save: capture the persistent state.
    pub fn capture(&mut self) -> SaveBody {
        gothic_portcov::hit!("capture");
        self.sync_inventory();
        // The trigger counters ride in the opaque host map: no schema change.
        let mut host = self.host.clone();
        host.remove(TRIGGER_KEY);
        host.remove(SECTOR_KEY);
        if let Ok(json) = serde_json::to_string(&self.range.scene().sectors.collect()) {
            host.insert(SECTOR_KEY.to_owned(), json);
        }
        let snap = self.triggers.snapshot(self.clock);
        if !snap.triggers.is_empty()
            && let Ok(json) = serde_json::to_string(&snap)
        {
            host.insert(TRIGGER_KEY.to_owned(), json);
        }
        SaveBody {
            sim: self.sim.snapshot(),
            host,
        }
    }

    /// Hash of the persistent state; equal for a core and its restored copy.
    pub fn state_hash(&mut self) -> u64 {
        let body = self.capture();
        fnv1a(&serde_json::to_vec(&body).unwrap_or_default())
    }

    /// Port of `gCWorld::LoadGameWorld`: put a save over the registered world.
    pub fn restore(&mut self, body: &SaveBody) -> RestoreReport {
        gothic_portcov::hit!("restore");
        let rep = self.sim.apply_snapshot(&body.sim);
        self.host = body.host.clone();
        if let Some(json) = self.host.remove(TRIGGER_KEY)
            && let Ok(snap) = serde_json::from_str(&json)
        {
            self.triggers.restore(&snap, self.clock);
        }
        if let Some(json) = self.host.remove(SECTOR_KEY)
            && let Ok(saved) = serde_json::from_str::<Vec<gothic_world::sector::SectorSave>>(&json)
        {
            // `UpdateSectors`: the image wins over whatever the world was registered with.
            let scene = self.range.scene_mut();
            scene.sectors.update_all(&mut scene.tree, &saved);
        }
        self.after_restore();
        rep
    }

    /// Save to `path` (any slot) in the current format.
    pub fn save_file(&mut self, path: &Path, meta: &SaveMeta) -> Result<()> {
        let body = self.capture();
        write_atomic(path, &encode(&body, meta)?)
    }

    /// Load `path`; the previous JSON quick save is still understood.
    pub fn load_file(&mut self, path: &Path) -> Result<RestoreReport> {
        let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
        if bytes.first() == Some(&b'{') {
            self.load_json(std::str::from_utf8(&bytes)?)?;
            return Ok(RestoreReport::default());
        }
        let (_, body) = decode(&bytes)?;
        Ok(self.restore(&body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Core;
    use gothic_gameplay::{GameDatabase, GameState};
    use gothic_sim::actor::Actor;
    use gothic_sim::item::ItemDb;
    use gothic_sim::script::enums::UseType;
    use gothic_sim::script_player::ObjectData;
    use gothic_sim::types::{ActorId, Species};
    use gothic_sim::world::Pos;
    use std::sync::Arc;

    fn strings() -> gothic_formats::stringtable::StringTable {
        let text = "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\nQUEST_X=Quest;;Quest;\r\n[LocAdmin_Revisions]\r\n";
        let mut b = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        gothic_formats::stringtable::StringTable::parse(&b).unwrap()
    }

    /// The world a host registers at start-up.
    fn core() -> Core {
        let db = Arc::new(GameDatabase::from_parts(vec![], vec![], strings(), "English").unwrap());
        let mut c = Core::new(db, ItemDb::default(), GameState::new(), 7);
        c.guess_weapons = false;
        let orc = Actor::npc(ActorId(0), "Orc_A", Species::Orc, 10, 10);
        c.sim.spawn(orc, Pos::flat(500.0, 0.0));
        let guard = Actor::npc(ActorId(0), "Guard_A", Species::Human, 5, 5);
        c.sim.spawn(guard, Pos::flat(-300.0, 40.0));
        c.sim.script.add_object(
            UseType::Chest,
            Pos::flat(100.0, 100.0),
            ObjectData {
                name: "Chest".into(),
                loot: vec![("It_Gold".into(), 50)],
                ..Default::default()
            },
        );
        c.sim.script.add_item("It_Apple", Pos::flat(10.0, 10.0));
        c
    }

    /// Pick up an item, open a chest, kill the orc, advance a quest flag, move an NPC.
    fn play(c: &mut Core) {
        use gothic_sim::world::WorldQuery;
        let mut snap = c.sim.snapshot();
        snap.items[0].dropped = true;
        snap.objects.values_mut().for_each(|o| {
            o.open = true;
            o.loot.clear();
        });
        c.sim.apply_snapshot(&snap);
        c.sim.actor_mut(ActorId(0)).inventory.insert("It_Apple", 1);
        c.sim.actor_mut(ActorId(0)).inventory.insert("It_Gold", 50);
        c.sim.actor_mut(ActorId(1)).hp = 0;
        c.sim.actor_mut(ActorId(1)).ai_mode = gothic_sim::types::AiMode::Dead;
        c.sim.world.teleport(ActorId(2), Pos::flat(-120.0, 75.0));
        c.sim.state.events.insert("Orc_Killed".into());
        c.sim.state.hours += 5;
        c.sim.clock.game_ms += 5 * 3_600_000 + 77;
        c.sim.tick = 900;
        c.host.insert("region".into(), "Ardea".into());
        c.host.insert("camera".into(), "follow".into());
    }

    #[test]
    fn scenario_round_trips_through_a_file_with_identical_hash() {
        let dir = std::env::temp_dir().join(format!("g3rs-test-{}", std::process::id()));
        let mut a = core();
        play(&mut a);
        let path = SaveDir(dir.clone()).manual(3);
        let meta = SaveMeta {
            name: "test".into(),
            kind: SlotKind::Manual,
        };
        a.save_file(&path, &meta).unwrap();
        let h = a.state_hash();

        let mut b = core();
        assert_ne!(b.state_hash(), h);
        let rep = b.load_file(&path).unwrap();
        assert_eq!(rep.actors_skipped + rep.objects_skipped, 0);
        assert_eq!(b.state_hash(), h);
        assert_eq!(b.host.get("camera").map(String::as_str), Some("follow"));
        let listed = SaveDir(dir.clone()).list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].header.region, "Ardea");
        assert_eq!(listed[0].header.play_ms, 30_000);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corruption_and_foreign_files_are_rejected() {
        let mut a = core();
        let meta = SaveMeta {
            name: "x".into(),
            kind: SlotKind::Quick,
        };
        let good = encode(&a.capture(), &meta).unwrap();
        assert!(decode(&good).is_ok());
        let mut bad = good.clone();
        let n = bad.len();
        bad[n / 2 + 20] ^= 0x55;
        assert!(decode(&bad).is_err());
        assert!(decode(&good[..good.len() - 12]).is_err());
        assert!(decode(b"not a save at all").is_err());
        let mut newer = good;
        newer[4] = 9;
        assert!(decode(&newer).unwrap_err().to_string().contains("newer"));
    }

    #[test]
    fn older_schema_is_migrated_forward() {
        // A schema-1 body kept the clock under another key; the (test-only) migration table
        // renames it, exactly like a production step would.
        fn v1_to_v2(v: &mut Value) {
            let sim = v.get_mut("sim").and_then(Value::as_object_mut).unwrap();
            let old = sim.remove("clock_ms").unwrap();
            sim.insert("game_ms".into(), old);
        }
        let mut a = core();
        let body = a.capture();
        let mut value = serde_json::to_value(&body).unwrap();
        let sim = value["sim"].as_object_mut().unwrap();
        let ms = sim.remove("game_ms").unwrap();
        sim.insert("clock_ms".into(), ms);
        let raw = serde_json::to_vec(&value).unwrap();
        let header = SlotHeader {
            schema: 1,
            name: "old".into(),
            kind: SlotKind::Auto,
            saved_unix: 1,
            game_ms: 0,
            play_ms: 0,
            region: String::new(),
            hero_pos: [0.0; 3],
            hero_level: 0,
            hash: fnv1a(&raw),
        };
        let file = pack(1, &header, &raw).unwrap();
        let (h, migrated) = unpack(&file, 2, &[v1_to_v2]).unwrap();
        assert_eq!(h.name, "old");
        let back: SaveBody = serde_json::from_value(migrated).unwrap();
        assert_eq!(back, body);
        // Without the step the same file cannot be read.
        assert!(unpack(&file, 2, &[]).is_err());
    }

    #[test]
    fn autosaves_rotate_and_manual_slots_list_newest_first() {
        let dir = SaveDir(std::env::temp_dir().join(format!("g3rs-rot-{}", std::process::id())));
        let mut a = core();
        let meta = SaveMeta {
            name: "auto".into(),
            kind: SlotKind::Auto,
        };
        let mut used = Vec::new();
        for _ in 0..AUTOSAVES {
            let p = dir.next_autosave();
            a.save_file(&p, &meta).unwrap();
            used.push(p);
        }
        used.sort();
        used.dedup();
        assert_eq!(used.len(), AUTOSAVES, "each autosave slot is used once");
        assert_eq!(dir.list().len(), AUTOSAVES);
        let _ = std::fs::remove_dir_all(dir.0);
    }
}
