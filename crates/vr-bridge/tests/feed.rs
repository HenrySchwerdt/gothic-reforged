//! The VR UI against the real `gothic-runtime` core (synthetic data): view models from core
//! state, window actions applied to the core, and a full trade through the VR window.
use bevy::prelude::*;
use gothic_gameplay::inventory::StackType;
use gothic_gameplay::model::QuestStatus;
use gothic_gameplay::state::{LogEntry, LogKind};
use gothic_gameplay::{GameDatabase, GameState};
use gothic_runtime::{Command, Core, Event};
use gothic_sim::actor::Actor;
use gothic_sim::item::{ItemDb, ItemDef};
use gothic_sim::types::{ActorId, Species};
use gothic_sim::world::Pos;
use gothic_ui::PageEvent;
use gothic_vr::{Hand, Pose, VrInput, XrFrame};
use gothic_vr_bridge::feed::{self, Applied, OpenWindow};
use gothic_vr_ui::hud::Tracked;
use gothic_vr_ui::{VrWindow, WindowAction, WindowView};
use std::sync::Arc;

fn def(name: &str, category: &str, use_type: Option<&str>, value: i32) -> ItemDef {
    ItemDef {
        name: name.into(),
        category: category.into(),
        use_type: use_type.map(String::from),
        value,
        quality: 0,
        robe: false,
        damage: None,
        requirements: Vec::new(),
        required_skills: Vec::new(),
        modifiers: Vec::new(),
        spell: None,
        permanent: false,
        mission_item: false,
        activate_skills: Vec::new(),
        skill: None,
    }
}

fn core() -> (Core, ActorId) {
    let text = "[LocAdmin_Languages]\r\nLanguages=English\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n[LocAdmin_Revisions]\r\n";
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    let strings = gothic_formats::stringtable::StringTable::parse(&b).unwrap();
    let db = Arc::new(GameDatabase::from_parts(vec![], vec![], strings, "English").unwrap());
    let mut items = ItemDb::default();
    items.insert(def("It_1H_Sword_01", "Weapon", Some("1H"), 1000));
    items.insert(def("It_Gold", "Misc", None, 1));
    let mut c = Core::new(db, items, GameState::new(), 7);
    c.guess_weapons = false;
    let smith = c
        .sim
        .spawn(Actor::npc(ActorId(0), "Hamlar_Smith", Species::Human, 5, 5), Pos::flat(100.0, 0.0));
    let info = |c: &Core, t: &str| c.sim.items.get(t).map(|d| Arc::new(d.to_info()));
    let (sw, gd) = (info(&c, "It_1H_Sword_01"), info(&c, "It_Gold"));
    c.sim.actors[smith.0 as usize]
        .inventory
        .add_items("It_1H_Sword_01", sw, 1, 0, StackType::Merchandise);
    let hero = c.hero;
    c.sim.actors[hero.0 as usize]
        .inventory
        .add_items("It_Gold", gd, 1500, 0, StackType::Normal);
    (c, smith)
}

#[test]
fn trade_view_actions_and_deal_follow_the_core() {
    let (mut c, smith) = core();
    let v = feed::trade_view(&mut c, smith, "");
    assert_eq!(v.vendor, "Hamlar Smith");
    assert_eq!(v.vendor_rows.len(), 1);
    assert_eq!(v.player.gold, 1500);
    assert!(!v.deal);
    // Add the sword: the friendly price is 900, as the sim's own test says.
    let key = v.vendor_rows[0].key;
    let o = feed::apply_trade_event(&mut c, smith, &PageEvent::TradeAdd { player_side: false, key }, 1);
    assert!(o.changed && !o.refused);
    let v = feed::trade_view(&mut c, smith, "");
    assert_eq!(v.ask_total, 900);
    assert_eq!(v.ask_rows.len(), 1);
    assert!(v.balance);
    feed::apply_trade_event(&mut c, smith, &PageEvent::TradeBalance, 1);
    let v = feed::trade_view(&mut c, smith, "");
    assert!(v.deal, "{v:?}");
    let o = feed::apply_trade_event(&mut c, smith, &PageEvent::TradeDeal, 1);
    assert!(o.dealt);
    let hero = feed::hero_panel(&c);
    assert_eq!(hero.gold, 600);
    assert!(hero.rows.iter().any(|r| r.item.eq_ignore_ascii_case("It_1H_Sword_01")));
}

#[test]
fn use_stack_loot_and_craft_are_the_desktop_commands() {
    let (c, _) = core();
    let panel = feed::hero_panel(&c);
    let gold = panel.rows.iter().find(|r| r.item == "It_Gold").unwrap();
    let dbg = |c: Option<Command>| format!("{c:?}");
    assert_eq!(dbg(feed::use_stack(&c, gold.key)), dbg(Some(Command::UseItem { template: "It_Gold".into() })));
    assert_eq!(
        dbg(feed::loot_command(9, &PageEvent::LootTake { key: 2 })),
        dbg(Some(Command::TakeLoot { object: 9, index: Some(2) }))
    );
    assert_eq!(
        dbg(feed::loot_command(9, &PageEvent::LootTakeAll)),
        dbg(Some(Command::TakeLoot { object: 9, index: None }))
    );
    assert_eq!(
        dbg(feed::craft_command(&[40, 41], &PageEvent::Craft { key: 1 })),
        dbg(Some(Command::Craft { recipe: 41 }))
    );
    let l = feed::loot_view(&c, "Alte_Truhe", &[("It_Gold".into(), 30, String::new())], true);
    assert_eq!((l.title.as_str(), l.pool_gold, l.take_all), ("Alte Truhe", Some(30), true));
}

#[test]
fn quest_log_reads_the_game_state() {
    let (mut c, _) = core();
    c.sim.state.set_quest_status("Q_Wolf", QuestStatus::Running);
    c.sim.state.set_quest_status("Q_Done", QuestStatus::Running);
    c.sim.state.set_quest_status("Q_Done", QuestStatus::Success);
    c.sim.state.log.push(LogEntry {
        quest: "q_wolf".into(),
        kind: LogKind::Dialog("Der Jaeger sucht den Wolf.".into()),
    });
    let q = feed::quest_log(&c);
    let active: Vec<_> = q.quests.iter().filter(|e| e.tab == 0).collect();
    let done: Vec<_> = q.quests.iter().filter(|e| e.tab == 1).collect();
    assert_eq!((active.len(), done.len()), (1, 1));
    assert_eq!(active[0].log[0], "Der Jaeger sucht den Wolf.", "newest first");
    assert!(active[0].log.iter().any(|l| l.contains("begonnen")));
    assert!(!q.clock.is_empty());
    let ch = feed::character(&c);
    assert_eq!(ch.level, c.state().level);
    assert!(ch.vitals.health.1 > 0.0);
}

/// The whole path: core state -> `WindowView` -> `VrWindow` -> laser + trigger -> `WindowAction`
/// -> `feed::apply_action` -> core -> refreshed view.
#[test]
fn a_trade_played_through_the_vr_window_changes_the_core() {
    let (mut c, smith) = core();
    let name = |t: &str| gothic_vr_ui::views::guess_name(t);
    let labels = gothic_vr_ui::views::Labels::default();
    let namer = gothic_vr_ui::views::Ctx { name: &name, labels: &labels };
    let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    let mut win = VrWindow::open(WindowView::Trade(feed::trade_view(&mut c, smith, "")), &head, &namer);
    let mut input = VrInput::new();
    let mut frame = XrFrame::default();
    let from = Vec3::new(0.25, 1.3, -0.3);
    input.push(frame);
    let open = OpenWindow::Trade { npc: smith };
    let mut press = |c: &mut Core, win: &mut VrWindow, text: &str| -> Vec<Applied> {
        let k = win.sheet.items.iter().position(|i| i.text == text).unwrap_or_else(|| panic!("no {text}"));
        let to = win.panel.point(win.sheet.items[k].rect.center());
        let rot = Quat::from_rotation_arc(Vec3::NEG_Z, (to - from).normalize());
        frame.hands[Hand::Right.index()].pose = Some(Pose::new(from, rot));
        frame.hands[Hand::Right.index()].trigger = 0.0;
        input.push(frame);
        win.update(&input, &Tracked::from_input(&input), Hand::Right, &namer);
        frame.hands[Hand::Right.index()].trigger = 1.0;
        input.push(frame);
        let acts = win.update(&input, &Tracked::from_input(&input), Hand::Right, &namer);
        let applied: Vec<_> = acts.iter().map(|a: &WindowAction| feed::apply_action(c, &open, a, 1)).collect();
        win.set_view(WindowView::Trade(feed::trade_view(c, smith, "")), &namer);
        applied
    };
    let a = press(&mut c, &mut win, "1H Sword 01");
    assert!(matches!(a[0], Applied::Trade(ref o) if o.changed && !o.refused), "{a:?}");
    press(&mut c, &mut win, "Gold-Ausgleich");
    let a = press(&mut c, &mut win, "Tauschen");
    assert!(matches!(a[0], Applied::Trade(ref o) if o.dealt), "{a:?}");
    assert_eq!(feed::hero_panel(&c).gold, 600);
}

fn quest_with_folder(name: &str, folder: &str) -> gothic_formats::quest::Quest {
    let text = format!(
        "[Quest]\r\nName={name}\r\nType=1\r\nFinishedQuests=\r\nLogTopic=QUEST_{name}\r\nExperiencePoints=0\r\nFolder={folder}\r\nDeliveryEntities=\r\nDeliveryAmounts=\r\nDeliveryCounter\r\nDestinationEntity=\r\n"
    );
    gothic_formats::quest::Quest::parse(text.as_bytes()).unwrap()
}

#[test]
fn quest_locations_come_from_the_quest_template_folder() {
    let text = "[LocAdmin_Languages]\r\nLanguages=English\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n[LocAdmin_Revisions]\r\n";
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    let strings = gothic_formats::stringtable::StringTable::parse(&b).unwrap();
    let quests = vec![
        quest_with_folder("Q_Braga", "Varant/Braga/"),
        quest_with_folder("Q_Region", "Varant/"),
        quest_with_folder("Q_Ardea", "Myrtana/Ardea/Hunter/"),
    ];
    let db = Arc::new(GameDatabase::from_parts(quests, vec![], strings, "English").unwrap());
    let mut c = Core::new(db, ItemDb::default(), GameState::new(), 1);
    assert_eq!(feed::quest_location(&c, "Q_Braga").as_deref(), Some("braga"));
    assert_eq!(feed::quest_location(&c, "Q_Ardea").as_deref(), Some("ardea"), "innermost known town");
    assert_eq!(feed::quest_location(&c, "Q_Region"), None);
    c.sim.state.set_quest_status("Q_Braga", QuestStatus::Running);
    c.sim.state.set_quest_status("Q_Ardea", QuestStatus::Success);
    c.sim.state.set_quest_status("Q_Region", QuestStatus::Running);
    assert_eq!(feed::locations_with_running_quest(&c), ["braga"]);
}

#[test]
fn sleeping_is_a_core_command_that_advances_the_clock_and_restores_the_hero() {
    let (mut c, _) = core();
    let hero = c.hero;
    let max = c.sim.actor(hero).hp_max(&c.sim.cfg);
    c.sim.actors[hero.0 as usize].hp = 10;
    let h0 = c.sim.clock.hour_of_day();
    let ev = c.tick(0.033, &[feed::wait_command(3)]);
    assert!(
        ev.iter().any(|e| matches!(e, Event::Player(gothic_sim::script::player::api::PlayerEvent::Slept { hours }) if (*hours - 3.0).abs() < 1e-3)),
        "{ev:?}"
    );
    assert_eq!(c.sim.actor(hero).hp, max, "PlayerSleep restores the hero");
    let h1 = c.sim.clock.hour_of_day();
    assert!(((h1 - h0 + 24.0) % 24.0 - 3.0).abs() < 0.05, "{h0} -> {h1}");
    assert_eq!(c.state().hours, c.sim.clock.total_hours(), "the quest clock follows the world clock");
    // The bed: sleep until the wake-up hour, whatever the hour is.
    let ev = c.tick(0.033, &[Command::Sleep { hours: None }]);
    assert!(ev.iter().any(|e| matches!(e, Event::Player(gothic_sim::script::player::api::PlayerEvent::Slept { .. }))));
    assert!((c.sim.clock.hour_of_day() - 9.0).abs() < 0.05, "{}", c.sim.clock.hour_of_day());
}

#[test]
fn the_dialog_sleep_command_goes_through_the_same_port() {
    let (mut c, _) = core();
    let hero = c.hero;
    c.sim.actors[hero.0 as usize].hp = 5;
    let h0 = c.sim.clock.hour_of_day();
    c.sim.apply_outputs(&[gothic_gameplay::script::Output::Effect {
        command: "Sleep".into(),
        entity1: String::new(),
        entity2: String::new(),
        arg1: "6".into(),
        arg2: String::new(),
    }]);
    assert_eq!(c.sim.actor(hero).hp, c.sim.actor(hero).hp_max(&c.sim.cfg));
    assert!(((c.sim.clock.hour_of_day() - h0 + 24.0) % 24.0 - 6.0).abs() < 0.05);
}
