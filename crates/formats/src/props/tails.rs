//! Class-specific data that follows the property list and the class version (the "tail").
//! Layouts for the classes below were cross-checked against the shipped data (strict: any
//! mismatch makes the tail [`Tail::Raw`] and is counted in `DecodeStats::tail_errors`).
use crate::propstream::{
    Cursor, DecodeStats, PropValue, PropertySet, Strings, hex, read_sub_class,
};
use anyhow::{Result, ensure};
use serde::Serialize;

/// One item stack inside `gCInventory_PS` (`gCInventoryStack` sub-object).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InventoryStack {
    /// Template GUID of the item (resolve through the template GUID index).
    pub template: Option<String>,
    /// Concrete item entity GUID, if the stack is materialised.
    pub item: Option<String>,
    pub amount: i32,
    pub quality: i32,
    pub quick_slot: i32,
    pub generated: i32,
    pub set: Box<PropertySet>,
}

/// Script line stored on a `gCItem_PS` (use effects).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScriptLine {
    pub command: String,
    pub entity1: String,
    pub entity2: String,
    pub id1: String,
    pub id2: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnimationSlot {
    pub name: Option<String>,
    pub file: String,
    pub switch: i32,
    pub file2: Option<String>,
    pub switch2: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum Tail {
    /// No bytes after the class version.
    Empty,
    /// Not decoded (unknown class layout); `len` bytes were skipped.
    Raw {
        len: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Common `eCEntityPropertySet` tail: persistence version and a flag.
    PropertySetFlag {
        version: u16,
        flag: bool,
    },
    Inventory {
        stacks: Vec<InventoryStack>,
        /// (slot enum index, template GUID, item GUID) for occupied slots.
        slots: Vec<(usize, Option<String>, Option<String>)>,
    },
    Item {
        slot: i32,
        visible: bool,
        script: Vec<ScriptLine>,
        script_executed: bool,
    },
    Animation {
        fxa: AnimationSlot,
        body_parts: Vec<AnimationSlot>,
        st_entries: Vec<String>,
        attachments: Vec<(Option<String>, String)>,
        boundary: [[f32; 3]; 2],
        trailing: usize,
    },
    EntityList {
        members: Vec<Option<String>>,
    },
}

pub fn decode_tail(
    class: &str,
    class_version: Option<u16>,
    body: &mut Cursor,
    strings: &Strings,
    depth: usize,
    stats: &mut DecodeStats,
) -> Tail {
    let len = body.remaining();
    stats.tails_total += 1;
    stats.tails_by_class.entry(class.to_owned()).or_default().0 += 1;
    if class_version.is_none() || len == 0 {
        stats.tails_by_class.entry(class.to_owned()).or_default().1 += 1;
        stats.tails_decoded += 1;
        return Tail::Empty;
    }
    let start = body.pos;
    let result: Option<Result<Tail>> = match class {
        "gCInventory_PS" => Some(inventory(body, strings, depth, stats)),
        "gCItem_PS" => Some(item(body, strings)),
        "eCVisualAnimation_PS" => Some(animation(body, strings)),
        "gCParty_PS" | "gCEnclave_PS" | "gCAnchor_PS" => Some(entity_list(body)),
        _ => None,
    };
    match result {
        Some(Ok(tail)) => {
            stats.tails_by_class.entry(class.to_owned()).or_default().1 += 1;
            stats.tails_decoded += 1;
            body.pos = body.limit;
            tail
        }
        Some(Err(e)) => {
            body.pos = start;
            let msg = format!("{e:#}");
            *stats
                .tail_errors
                .entry(format!("{class}: {msg}"))
                .or_default() += 1;
            Tail::Raw {
                len,
                error: Some(msg),
            }
        }
        None => {
            if len == 3 {
                let b = &body.bytes[body.pos..body.pos + 3];
                if b[2] <= 1 {
                    let version = u16::from_le_bytes([b[0], b[1]]);
                    let flag = b[2] == 1;
                    stats.tails_by_class.entry(class.to_owned()).or_default().1 += 1;
                    stats.tails_decoded += 1;
                    return Tail::PropertySetFlag { version, flag };
                }
            }
            Tail::Raw { len, error: None }
        }
    }
}

fn proxy(body: &mut Cursor) -> Result<Option<String>> {
    let _version = body.u16()?;
    Ok(if body.flag()? {
        Some(hex(&body.guid()?))
    } else {
        None
    })
}

fn entity_list(body: &mut Cursor) -> Result<Tail> {
    body.skip(1)?;
    let n = body.u32()? as usize;
    ensure!(n <= body.remaining(), "entity list count");
    let members = (0..n).map(|_| proxy(body)).collect::<Result<Vec<_>>>()?;
    Ok(Tail::EntityList { members })
}

fn ref_of(set: &PropertySet, name: &str) -> Option<String> {
    match set.get(name) {
        Some(PropValue::EntityRef(g)) => g.clone(),
        _ => None,
    }
}

fn inventory(
    body: &mut Cursor,
    strings: &Strings,
    depth: usize,
    stats: &mut DecodeStats,
) -> Result<Tail> {
    body.skip(2)?;
    let n = body.u32()? as usize;
    ensure!(n <= 100_000 && n <= body.remaining(), "stack count");
    let mut stacks = Vec::with_capacity(n);
    for _ in 0..n {
        let set = read_sub_class(body, strings, depth, stats)?;
        stacks.push(InventoryStack {
            template: ref_of(&set, "Template"),
            item: ref_of(&set, "Item"),
            amount: set.int("Amount").unwrap_or(0),
            quality: set.int("Quality").unwrap_or(0),
            quick_slot: set.int("QuickSlot").unwrap_or(-1),
            generated: set.int("Generated").unwrap_or(0),
            set: Box::new(set),
        });
    }
    body.skip(2)?;
    let slot_count = body.u32()? as usize;
    ensure!(slot_count <= 256, "slot count {slot_count}");
    let mut slots = Vec::new();
    for i in 0..slot_count {
        ensure!(body.remaining() >= 3, "slot truncated");
        if body.bytes[body.pos + 2] == 1 {
            let set = read_sub_class(body, strings, depth, stats)?;
            slots.push((i, ref_of(&set, "Template"), ref_of(&set, "Item")));
        } else {
            ensure!(body.take(3)? == [1, 0, 0], "expected empty slot");
        }
    }
    // Cached treasure-set entity proxies (5), only populated in saved dumps.
    for _ in 0..5 {
        proxy(body)?;
    }
    Ok(Tail::Inventory { stacks, slots })
}

fn item(body: &mut Cursor, strings: &Strings) -> Result<Tail> {
    body.skip(24)?; // constant stats block
    let slot = body.i32()?;
    let visible = body.u8()? & 1 != 0;
    body.skip(1)?; // list prefix byte
    let n = body.u32()? as usize;
    ensure!(n <= 4096 && n <= body.remaining(), "script line count");
    let mut script = Vec::with_capacity(n);
    for _ in 0..n {
        let _version = body.u16()?;
        script.push(ScriptLine {
            command: strings.entry(body)?,
            entity1: strings.entry(body)?,
            entity2: strings.entry(body)?,
            id1: strings.entry(body)?,
            id2: strings.entry(body)?,
        });
    }
    let script_executed = body.u8()? & 1 != 0;
    Ok(Tail::Item {
        slot,
        visible,
        script,
        script_executed,
    })
}

fn anim_slot(body: &mut Cursor, strings: &Strings, named: bool) -> Result<AnimationSlot> {
    let name = if named {
        Some(strings.entry(body)?)
    } else {
        None
    };
    body.skip(2)?;
    let file = strings.entry(body)?;
    let switch = body.i32()?;
    let (file2, switch2) = if body.flag()? {
        body.skip(2)?;
        (Some(strings.entry(body)?), body.i32()?)
    } else {
        (None, 0)
    };
    Ok(AnimationSlot {
        name,
        file,
        switch,
        file2,
        switch2,
    })
}

fn animation(body: &mut Cursor, strings: &Strings) -> Result<Tail> {
    body.skip(2)?;
    let fxa = anim_slot(body, strings, false)?;
    let n = body.u32()? as usize;
    ensure!(n <= 4096 && n <= body.remaining(), "body part count");
    let mut body_parts = Vec::with_capacity(n);
    for _ in 0..n {
        body_parts.push(anim_slot(body, strings, true)?);
        ensure!(body.flag()?, "body part terminator");
    }
    ensure!(body.flag()?, "animation marker");
    let st = body.u32()? as usize;
    ensure!(st <= 65_536 && st <= body.remaining(), "st count");
    let st_entries = (0..st)
        .map(|_| strings.entry(body))
        .collect::<Result<Vec<_>>>()?;
    let extra = body.u32()? as usize;
    ensure!(
        extra <= 4096 && extra <= body.remaining(),
        "attachment count"
    );
    let mut attachments = Vec::with_capacity(extra);
    for _ in 0..extra {
        body.skip(2)?;
        let guid = if body.flag()? {
            Some(hex(&body.guid()?))
        } else {
            None
        };
        let name = strings.entry(body)?;
        body.skip(72)?;
        attachments.push((guid, name));
    }
    let boundary = [body.floats()?, body.floats()?];
    let trailing = body.remaining();
    Ok(Tail::Animation {
        fxa,
        body_parts,
        st_entries,
        attachments,
        boundary,
        trailing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::propstream::testutil::{object, prop, sub_class};

    fn proxy_bytes(guid: Option<u8>) -> Vec<u8> {
        let mut b = vec![1, 0];
        match guid {
            Some(g) => {
                b.push(1);
                b.extend([g; 20]);
            }
            None => b.push(0),
        }
        b
    }

    fn run(class: &str, strings: &[&str], tail: &[u8], stats: &mut DecodeStats) -> Tail {
        let table: Vec<&[u8]> = strings.iter().map(|s| s.as_bytes()).collect();
        let st = Strings::new(&table);
        let mut cur = Cursor::new(tail);
        decode_tail(class, Some(5), &mut cur, &st, 0, stats)
    }

    #[test]
    fn inventory_tail_decodes_stacks_and_slots() {
        let strings = [
            "Amount",
            "int",
            "Template",
            "eCTemplateEntityProxy",
            "gCInventoryStack",
            "gCInventorySlot",
        ];
        let stack = object(
            &[
                prop(0, 1, &3i32.to_le_bytes()),
                prop(2, 3, &proxy_bytes(Some(0xaa))),
            ],
            1,
            &[],
        );
        let slot = object(&[prop(2, 3, &proxy_bytes(Some(0xbb)))], 1, &[]);
        let mut t = vec![1, 0];
        t.extend(1u32.to_le_bytes());
        t.extend(sub_class(4, &stack));
        t.extend([1, 0]);
        t.extend(3u32.to_le_bytes());
        t.extend([1, 0, 0]); // empty slot 0
        t.extend(sub_class(5, &slot)); // occupied slot 1
        t.extend([1, 0, 0]); // empty slot 2
        for _ in 0..5 {
            t.extend(proxy_bytes(None));
        }
        let mut stats = DecodeStats::default();
        match run("gCInventory_PS", &strings, &t, &mut stats) {
            Tail::Inventory { stacks, slots } => {
                assert_eq!(stacks.len(), 1);
                assert_eq!(stacks[0].amount, 3);
                assert_eq!(
                    stacks[0].template.as_deref(),
                    Some("aa".repeat(20).as_str())
                );
                assert_eq!(slots, vec![(1, Some("bb".repeat(20)), None)]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(stats.tails_by_class["gCInventory_PS"], (1, 1));
        // every truncation is reported as a counted Raw tail, never a panic
        for n in 1..t.len() {
            let mut stats = DecodeStats::default();
            assert!(
                matches!(
                    run("gCInventory_PS", &strings, &t[..n], &mut stats),
                    Tail::Raw { .. }
                ),
                "{n}"
            );
        }
    }

    #[test]
    fn item_tail_decodes_use_script() {
        let strings = ["RunQuest", "", "Quest_A"];
        let mut t = vec![0u8; 24];
        t.extend(7i32.to_le_bytes());
        t.push(1);
        t.push(0);
        t.extend(1u32.to_le_bytes());
        t.extend([1, 0]);
        for i in [0u16, 1, 1, 2, 1] {
            t.extend(i.to_le_bytes());
        }
        t.push(1);
        let mut stats = DecodeStats::default();
        match run("gCItem_PS", &strings, &t, &mut stats) {
            Tail::Item {
                slot,
                visible,
                script,
                script_executed,
            } => {
                assert_eq!((slot, visible, script_executed), (7, true, true));
                assert_eq!(script[0].command, "RunQuest");
                assert_eq!(script[0].id1, "Quest_A");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn animation_tail_decodes_body_parts() {
        let strings = ["skel.fxa", "Slot_Head", "head.fxa", "Slot_Body", "body.fxa"];
        let mut t = vec![5, 0];
        t.extend([4, 0]);
        t.extend(0u16.to_le_bytes());
        t.extend(0i32.to_le_bytes());
        t.push(0);
        t.extend(2u32.to_le_bytes());
        for (name, file, sw) in [(1u16, 2u16, 29i32), (3, 4, 0)] {
            t.extend(name.to_le_bytes());
            t.extend([4, 0]);
            t.extend(file.to_le_bytes());
            t.extend(sw.to_le_bytes());
            t.push(0);
            t.push(1);
        }
        t.push(1);
        t.extend(0u32.to_le_bytes());
        t.extend(0u32.to_le_bytes());
        for f in [0.0f32, 0.0, 0.0, 1.0, 2.0, 3.0] {
            t.extend(f.to_le_bytes());
        }
        let mut stats = DecodeStats::default();
        match run("eCVisualAnimation_PS", &strings, &t, &mut stats) {
            Tail::Animation {
                fxa,
                body_parts,
                boundary,
                trailing,
                ..
            } => {
                assert_eq!(fxa.file, "skel.fxa");
                assert_eq!(body_parts.len(), 2);
                assert_eq!(body_parts[0].name.as_deref(), Some("Slot_Head"));
                assert_eq!(
                    (body_parts[0].file.as_str(), body_parts[0].switch),
                    ("head.fxa", 29)
                );
                assert_eq!(boundary[1], [1.0, 2.0, 3.0]);
                assert_eq!(trailing, 0);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unknown_classes_are_raw_and_flag_tails_recognised() {
        let mut stats = DecodeStats::default();
        assert_eq!(
            run("eCSomething_PS", &[], &[1, 2, 3, 4, 5], &mut stats),
            Tail::Raw {
                len: 5,
                error: None
            }
        );
        assert_eq!(
            run("gCDialog_PS", &[], &[2, 0, 1], &mut stats),
            Tail::PropertySetFlag {
                version: 2,
                flag: true
            }
        );
        assert_eq!(stats.tails_by_class["eCSomething_PS"], (1, 0));
    }
}
