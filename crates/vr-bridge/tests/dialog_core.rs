//! The VR dialogue flow driven by a real `Core` dialogue (synthetic infos, no game data).
use gothic_formats::{info::Info, stringtable::StringTable};
use gothic_gameplay::{GameDatabase, GameState};
use gothic_runtime::{Command, Core, Event};
use gothic_sim::actor::Actor;
use gothic_sim::item::ItemDb;
use gothic_sim::types::{ActorId, Species};
use gothic_sim::world::Pos;
use gothic_vr_bridge::dialog_flow::{DialogFlow, FlowOut};
use std::sync::Arc;

type Step<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, &'a str);

fn info(name: &str, owner: &str, script: &[Step]) -> Info {
    let col = |f: &dyn Fn(&Step<'_>) -> String| {
        let mut s = script.iter().map(f).collect::<Vec<_>>().join(";");
        s.push(';');
        s
    };
    let text = format!(
        "[Info]\r\nName={name}\r\nSortID={}\r\nOwner={owner}\r\nInfoGiven=false\r\nParent=\r\nQuest=\r\nConditionType=3\r\nInfoType=3\r\nCondPlayerKnows=\r\nInfoScript_Commands={}\r\nInfoScript_Entities1={}\r\nInfoScript_Entities2={}\r\nInfoScript_IDs1={}\r\nInfoScript_IDs2={}\r\nInfoScript_Texts={}\r\n",
        name.len() * 10 + script.len(),
        script.iter().map(|s| s.0).collect::<Vec<_>>().join(";"),
        col(&|s| s.1.to_string()),
        col(&|s| s.2.to_string()),
        col(&|s| s.3.to_string()),
        col(&|s| s.4.to_string()),
        col(&|s| s.5.to_string()),
    );
    Info::parse(text.as_bytes()).unwrap()
}

fn strings() -> StringTable {
    let mut keys = String::new();
    for (k, en) in [
        ("INFO_ASK", "Gibt es Arbeit?"),
        ("INFO_WORK", "Ja, bring diesen Brief zu Bob."),
        ("INFO_BYE", "Auf Wiedersehen."),
    ] {
        keys.push_str(&format!("{k}={en};;{en};\r\n"));
    }
    let text = format!(
        "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n{keys}[LocAdmin_Revisions]\r\n"
    );
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    StringTable::parse(&b).unwrap()
}

fn core() -> (Core, ActorId) {
    let infos = vec![
        info(
            "H1",
            "Hamlar_Smith",
            &[
                ("Say", "player", "", "", "", "INFO_ASK"),
                ("Say", "npc", "", "", "", "INFO_WORK"),
            ],
        ),
        info("H2", "Hamlar_Smith", &[("Say", "player", "", "", "", "INFO_BYE")]),
    ];
    let db = Arc::new(GameDatabase::from_parts(vec![], infos, strings(), "German").unwrap());
    let mut c = Core::new(db, ItemDb::default(), GameState::new(), 3);
    let npc = c
        .sim
        .spawn(Actor::npc(ActorId(0), "Hamlar_Smith", Species::Human, 5, 5), Pos::flat(100.0, 0.0));
    (c, npc)
}

/// Run the flow over the events of one tick; returns what it asked the host to do.
fn pump(f: &mut DialogFlow, events: &[Event]) -> Vec<FlowOut> {
    for e in events {
        f.on_event(e);
    }
    let mut out = Vec::new();
    // Skip every line (the trigger) until the flow is idle.
    for _ in 0..20 {
        let o = f.tick(0.016);
        let say = o.iter().any(|o| matches!(o, FlowOut::Say { .. }));
        out.extend(o);
        if say {
            f.finish_line();
        } else {
            break;
        }
    }
    out
}

#[test]
fn a_real_core_dialogue_runs_through_the_vr_flow() {
    let (mut c, npc) = core();
    let mut f = DialogFlow::default();
    let ev = c.tick(0.033, &[Command::Talk { npc }]);
    let out = pump(&mut f, &ev);
    let options = out.iter().find_map(|o| match o {
        FlowOut::ShowOptions(o) => Some(o.clone()),
        _ => None,
    });
    let options = options.unwrap_or_else(|| panic!("no options: {out:?} / {ev:?}"));
    assert!(options.len() >= 2, "{options:?}");
    assert!(f.npc == Some(npc));
    // Pick the first info: the core speaks the hero's line and the NPC's answer.
    let (cmd, hide) = f.choose(0).expect("choose");
    assert_eq!(hide, FlowOut::HideOptions);
    let ev = c.tick(0.033, &[cmd]);
    let said: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::LineSpoken { text, player, .. } => Some((*player, text.clone().unwrap_or_default())),
            _ => None,
        })
        .collect();
    assert!(said.iter().any(|(p, t)| *p && t.contains("Arbeit")), "{said:?} / options {options:?} / {ev:?}");
    assert!(said.iter().any(|(p, t)| !*p && t.contains("Brief")), "{said:?}");
    let out = pump(&mut f, &ev);
    let says: Vec<_> = out
        .iter()
        .filter_map(|o| match o {
            FlowOut::Say { line, .. } => Some(line.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(says.len(), said.len(), "every spoken line is shown once, in order: {says:?}");
    assert!(says.iter().position(|t| t.contains("Arbeit")) < says.iter().position(|t| t.contains("Brief")));
    // The dialogue either went on (leave it) or the core ended it after the info: the panel
    // closes either way, after the pending lines.
    if !out.contains(&FlowOut::Closed) {
        let ev = c.tick(0.033, &[f.leave()]);
        let out = pump(&mut f, &ev);
        assert!(out.contains(&FlowOut::Closed), "{out:?} / {ev:?}");
    }
    assert!(!f.is_open());
}
