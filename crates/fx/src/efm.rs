//! `g3.efm`: the effect map. Port of `gCEffectMap::Read` (Game:0x1005f2e0) and
//! `gCEffectCommandSequence::Read` (Game:0x10051a80): named command sequences (`eff_item_torch_01`)
//! that the effect system plays when an entity with `gCEffect_PS` starts.
use anyhow::{Context, Result, ensure};
use gothic_formats::genome::GenomeFile;
use gothic_formats::propstream::{
    Cursor, DecodeStats, PropValue, PropertySet, Strings, read_sub_class,
};
use std::collections::HashMap;

/// How a spawned entity is attached (`gEEffectLink`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Link {
    /// `Independent`: placed once where the effect started and left alone.
    #[default]
    Independent,
    /// `TargetEntity`: follows the effect's owner every frame.
    TargetEntity,
    /// `TargetBone`: follows a bone of the owner.
    TargetBone,
}

impl Link {
    pub fn from_enum(v: i32) -> Self {
        match v {
            1 => Self::TargetEntity,
            2 => Self::TargetBone,
            _ => Self::Independent,
        }
    }
}

/// One effect command with its start time inside the sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectCommand {
    /// `TimeOffset`: seconds after the effect started.
    pub time: f32,
    pub kind: CommandKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommandKind {
    /// `gCEffectCommandSpawnEntity`: instantiate a template (usually a particle emitter).
    SpawnEntity {
        /// Template GUID (lowercase hex).
        template: Option<String>,
        /// Random offset box (`min`, `max`), cm, in the link's frame.
        offset: [[f32; 3]; 2],
        bone: String,
        link: Link,
        auto_kill: bool,
        entity_index: i32,
    },
    /// `gCEffectCommandPlaySound`: handled by the audio crate (sound list in the properties).
    PlaySound,
    /// `gCEffectCommandPlayVoice`.
    PlayVoice,
    /// `gCEffectCommandKillEntity`: kill the entity spawned by command `index`.
    KillEntity { index: i32 },
    /// `gCEffectCommandTriggerEntity`: fire a trigger on a spawned entity.
    TriggerEntity { index: i32, tag: String },
    /// `gCEffectCommandEarthquake`.
    Earthquake { amplitude: f32, duration: f32 },
    /// `gCEffectCommandMusicTrigger` / `ModifyEntity` / `CreateDecal` and anything unknown.
    Other(String),
}

/// A sorted command list (`gCEffectCommandSequence`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandSequence {
    pub commands: Vec<EffectCommand>,
}

impl CommandSequence {
    /// Port of gCEffectCommandSequence::GetEndTime (Game:0x100519f0): the largest start time (commands are
    /// sorted, so the last one).
    pub fn end_time(&self) -> f32 {
        gothic_portcov::hit!("end_time");
        self.commands.last().map_or(0.0, |c| c.time)
    }
}

#[derive(Debug, Default)]
pub struct EffectMap {
    sequences: HashMap<String, CommandSequence>,
}

impl EffectMap {
    pub fn len(&self) -> usize {
        self.sequences.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sequences.is_empty()
    }

    /// Port of gCEffectMap::Lookup (Game:0x1005ef30): names are matched case-insensitively.
    pub fn lookup(&self, name: &str) -> Option<&CommandSequence> {
        gothic_portcov::hit!("lookup");
        self.sequences.get(&name.to_ascii_lowercase())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sequences.keys().map(String::as_str)
    }

    /// Port of gCEffectMap::Read (Game:0x1005f2e0): `u16 version (2)`, `u32 count`, then per effect a name and a
    /// sequence (`u16 version (1)`, `u32 count`, `count` class instances).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        gothic_portcov::hit!("parse");
        let genome = GenomeFile::parse(bytes)?;
        let strings = Strings::new(&genome.strings);
        let mut cur = Cursor::new(genome.body);
        let version = cur.u16()?;
        ensure!(version == 2, "unsupported effect map version {version}");
        let count = cur.u32()? as usize;
        ensure!(count <= 100_000, "effect count out of range");
        let mut stats = DecodeStats::default();
        let mut sequences = HashMap::with_capacity(count);
        for i in 0..count {
            let name = strings
                .entry(&mut cur)
                .with_context(|| format!("effect {i} name"))?;
            let sequence = read_sequence(&mut cur, &strings, &mut stats)
                .with_context(|| format!("effect {name}"))?;
            // The engine warns about duplicates; the first entry stays.
            sequences
                .entry(name.to_ascii_lowercase())
                .or_insert(sequence);
        }
        Ok(Self { sequences })
    }
}

fn read_sequence(
    cur: &mut Cursor,
    strings: &Strings,
    stats: &mut DecodeStats,
) -> Result<CommandSequence> {
    let version = cur.u16()?;
    ensure!(
        version == 1,
        "unsupported command sequence version {version}"
    );
    let count = cur.u32()? as usize;
    ensure!(count <= 4096, "command count out of range");
    let mut commands = Vec::with_capacity(count);
    for _ in 0..count {
        let set = read_sub_class(cur, strings, 0, stats)?;
        commands.push(command_from_set(&set));
    }
    // CompareItems orders by start time.
    commands.sort_by(|a, b| a.time.total_cmp(&b.time));
    Ok(CommandSequence { commands })
}

fn command_from_set(set: &PropertySet) -> EffectCommand {
    let time = set.float("TimeOffset").unwrap_or(0.0);
    let kind = match set.class.as_str() {
        "gCEffectCommandSpawnEntity" => {
            let offset = match set.get("Offset") {
                Some(PropValue::Range3(r)) => *r,
                Some(PropValue::Vec3(v)) => [*v, *v],
                _ => [[0.0; 3]; 2],
            };
            let link = match set.get("CoordinateSystem") {
                Some(PropValue::Enum { value, .. }) => Link::from_enum(*value),
                _ => Link::Independent,
            };
            CommandKind::SpawnEntity {
                template: set.get("TemplateEntity").and_then(|v| match v {
                    PropValue::EntityRef(g) => g.clone(),
                    _ => None,
                }),
                offset,
                bone: set.string("BoneName").unwrap_or_default().to_owned(),
                link,
                auto_kill: set.boolean("AutoKill").unwrap_or(false),
                entity_index: set.int("EntityIndex").unwrap_or(0),
            }
        }
        "gCEffectCommandPlaySound" => CommandKind::PlaySound,
        "gCEffectCommandPlayVoice" => CommandKind::PlayVoice,
        "gCEffectCommandKillEntity" => CommandKind::KillEntity {
            index: set.int("EntityIndex").unwrap_or(0),
        },
        "gCEffectCommandTriggerEntity" => CommandKind::TriggerEntity {
            index: set.int("EntityIndex").unwrap_or(0),
            tag: set.string("Tag").unwrap_or_default().to_owned(),
        },
        "gCEffectCommandEarthquake" => CommandKind::Earthquake {
            amplitude: set.float("Amplitude").unwrap_or(0.0),
            duration: set.float("Duration").unwrap_or(0.0),
        },
        other => CommandKind::Other(other.to_owned()),
    };
    EffectCommand { time, kind }
}
