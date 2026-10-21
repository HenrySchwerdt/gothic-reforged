//! Synthetic quest database (no game data): ten interdependent quests covering the quest
//! types the bot has to drive (report, item delivery, kill, defeat, enter area, bring NPC),
//! prerequisite chains, game-event gates and a dialog tree. [`broken`] adds quests that
//! cannot be finished, for the stuck report.
//!
//! The records are written in the game's own INI text form and parsed by `gothic-formats`,
//! so the fixture exercises the same loader path as the real `Quests.pak` / `Infos.pak`.
use gothic_formats::{info::Info, quest::Quest, stringtable::StringTable};
use gothic_gameplay::GameDatabase;

/// One info-script line: (command, entity1, entity2, id1, id2, text key).
pub type Step<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, &'a str);

/// Condition types (`gEInfoCondType`).
pub mod cond {
    pub const GENERAL: u32 = 3;
    pub const ACTIVATOR: u32 = 6;
    pub const DELIVERY: u32 = 8;
    pub const SUCCESS: u32 = 10;
}

/// Info types (`gEInfoType`).
pub mod ty {
    pub const INFO: u32 = 3;
    pub const PARENT: u32 = 4;
}

/// Quest types (`gEQuestType`).
pub mod quest_type {
    pub const HAS_ITEMS: u32 = 0;
    pub const REPORT: u32 = 1;
    pub const KILL: u32 = 2;
    pub const DEFEAT: u32 = 3;
    pub const BRING_NPC: u32 = 6;
    pub const ENTER_AREA: u32 = 8;
}

/// An info record. `knows` is the `CondPlayerKnows` list (`;`-separated).
#[allow(clippy::too_many_arguments)]
pub fn info(
    name: &str,
    owner: &str,
    parent: &str,
    quest: &str,
    cond: u32,
    info_type: u32,
    knows: &str,
    script: &[Step],
) -> Info {
    let col = |f: &dyn Fn(&Step<'_>) -> String| {
        let mut s = script.iter().map(f).collect::<Vec<_>>().join(";");
        s.push(';');
        s
    };
    let text = format!(
        "[Info]\r\nName={name}\r\nSortID={}\r\nOwner={owner}\r\nInfoGiven=false\r\nParent={parent}\r\nQuest={quest}\r\nConditionType={cond}\r\nInfoType={info_type}\r\nCondPlayerKnows={knows}\r\nInfoScript_Commands={}\r\nInfoScript_Entities1={}\r\nInfoScript_Entities2={}\r\nInfoScript_IDs1={}\r\nInfoScript_IDs2={}\r\nInfoScript_Texts={}\r\n",
        name.len() * 10 + script.len(),
        script.iter().map(|s| s.0).collect::<Vec<_>>().join(";"),
        col(&|s| s.1.to_string()),
        col(&|s| s.2.to_string()),
        col(&|s| s.3.to_string()),
        col(&|s| s.4.to_string()),
        col(&|s| s.5.to_string()),
    );
    Info::parse(text.as_bytes()).expect("fixture info")
}

/// A quest record. `finished` and `delivery`/`amounts` are `;`-separated lists.
pub fn quest(
    name: &str,
    quest_type: u32,
    finished: &str,
    delivery: &str,
    amounts: &str,
    destination: &str,
) -> Quest {
    let text = format!(
        "[Quest]\r\nName={name}\r\nType={quest_type}\r\nFinishedQuests={finished}\r\nLogTopic=QUEST_{name}\r\nExperiencePoints=50\r\nDeliveryEntities={delivery}\r\nDeliveryAmounts={amounts}\r\nDeliveryCounter\r\nDestinationEntity={destination}\r\n"
    );
    Quest::parse(text.as_bytes()).expect("fixture quest")
}

/// String table with English and German texts for the given keys.
pub fn strings(entries: &[(&str, &str, &str)]) -> StringTable {
    let mut keys = String::new();
    for (k, en, de) in entries {
        keys.push_str(&format!("{k}={en};;{de};\r\n"));
    }
    let text = format!(
        "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n{keys}[LocAdmin_Revisions]\r\n"
    );
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    StringTable::parse(&b).expect("fixture strings")
}

fn say(key: &'static str) -> Step<'static> {
    ("Say", "npc", "", "", "", key)
}

fn ask(key: &'static str) -> Step<'static> {
    ("Say", "player", "", "", "", key)
}

fn event(e: &'static str) -> Step<'static> {
    ("SetGameEvent", "", "", e, "", "")
}

/// The ten fixture quests in a sensible play order.
pub const QUESTS: [&str; 10] = [
    "Q_Letter",
    "Q_Herbs",
    "Q_Wolves",
    "Q_Arena",
    "Q_Scout",
    "Q_Escort",
    "Q_Ring",
    "Q_Tribute",
    "Q_Mira",
    "Q_Final",
];

fn records() -> (Vec<Quest>, Vec<Info>) {
    use cond::*;
    use quest_type::*;
    use ty::*;
    let quests = vec![
        quest("Q_Letter", REPORT, "", "Bob", "1", ""),
        quest("Q_Herbs", HAS_ITEMS, "Q_Letter", "It_Herb", "3", "Bob"),
        quest("Q_Wolves", KILL, "", "Wolf", "2", ""),
        quest("Q_Arena", DEFEAT, "Q_Wolves", "Brutus", "1", ""),
        quest("Q_Scout", ENTER_AREA, "", "PC_Hero", "1", "Area_Pass"),
        quest("Q_Escort", BRING_NPC, "Q_Scout", "Farmer", "1", "Captain"),
        quest(
            "Q_Ring",
            HAS_ITEMS,
            "Q_Herbs;Q_Scout",
            "It_Ring",
            "1",
            "Merchant",
        ),
        quest("Q_Tribute", REPORT, "Q_Arena", "Chief", "1", ""),
        quest("Q_Mira", REPORT, "", "Mira", "1", ""),
        quest(
            "Q_Final",
            REPORT,
            "Q_Ring;Q_Tribute;Q_Escort;Q_Mira",
            "Alice",
            "1",
            "",
        ),
    ];
    let infos = vec![
        // Alice opens the chain with a gossip line that sets the event her quest needs.
        info(
            "A_Intro",
            "Alice",
            "",
            "",
            GENERAL,
            INFO,
            "",
            &[ask("T_HELLO"), say("T_TROUBLE"), event("Alice_Needs_Help")],
        ),
        info(
            "A_Letter",
            "Alice",
            "",
            "Q_Letter",
            ACTIVATOR,
            INFO,
            "Alice_Needs_Help",
            &[
                ask("T_WORK"),
                say("T_LETTER"),
                ("Give", "npc", "player", "It_Letter", "1", ""),
            ],
        ),
        info(
            "B_Letter",
            "Bob",
            "",
            "Q_Letter",
            DELIVERY,
            INFO,
            "",
            &[
                ask("T_GIVE"),
                ("Give", "player", "npc", "It_Letter", "1", ""),
                say("T_THANKS"),
            ],
        ),
        // Bob: herbs, after the letter.
        info(
            "B_Herbs",
            "Bob",
            "",
            "Q_Herbs",
            ACTIVATOR,
            INFO,
            "",
            &[ask("T_WORK"), say("T_HERBS")],
        ),
        info(
            "B_HerbsDone",
            "Bob",
            "",
            "Q_Herbs",
            DELIVERY,
            INFO,
            "",
            &[ask("T_GIVE"), say("T_THANKS")],
        ),
        // Gorn: wolves (kill), then the arena (defeat) once he trusts you.
        info(
            "G_Wolves",
            "Gorn",
            "",
            "Q_Wolves",
            ACTIVATOR,
            INFO,
            "",
            &[ask("T_WORK"), say("T_WOLVES")],
        ),
        info(
            "G_WolvesDone",
            "Gorn",
            "",
            "Q_Wolves",
            SUCCESS,
            INFO,
            "",
            &[say("T_THANKS"), event("Gorn_Trusts")],
        ),
        info(
            "G_Arena",
            "Gorn",
            "",
            "Q_Arena",
            ACTIVATOR,
            INFO,
            "Gorn_Trusts",
            &[ask("T_WORK"), say("T_ARENA")],
        ),
        info(
            "G_ArenaDone",
            "Gorn",
            "",
            "Q_Arena",
            SUCCESS,
            INFO,
            "",
            &[say("T_THANKS"), event("Arena_Champion")],
        ),
        // Captain: scouting (enter area), then the farmer escort (bring NPC).
        info(
            "C_Scout",
            "Captain",
            "",
            "Q_Scout",
            ACTIVATOR,
            INFO,
            "",
            &[ask("T_WORK"), say("T_SCOUT")],
        ),
        info(
            "C_ScoutDone",
            "Captain",
            "",
            "Q_Scout",
            SUCCESS,
            INFO,
            "",
            &[say("T_THANKS"), event("Captain_Sent_You")],
        ),
        info(
            "F_Escort",
            "Farmer",
            "",
            "Q_Escort",
            ACTIVATOR,
            INFO,
            "Captain_Sent_You",
            &[ask("T_HELLO"), say("T_ESCORT")],
        ),
        info(
            "C_Escort",
            "Captain",
            "",
            "Q_Escort",
            DELIVERY,
            INFO,
            "",
            &[say("T_THANKS")],
        ),
        // Merchant: ring, after herbs and scouting.
        info(
            "M_Ring",
            "Merchant",
            "",
            "Q_Ring",
            ACTIVATOR,
            INFO,
            "",
            &[ask("T_WORK"), say("T_RING")],
        ),
        info(
            "M_RingDone",
            "Merchant",
            "",
            "Q_Ring",
            DELIVERY,
            INFO,
            "",
            &[ask("T_GIVE"), say("T_THANKS")],
        ),
        // Chief: tribute for the arena champion.
        info(
            "K_Tribute",
            "Chief",
            "",
            "Q_Tribute",
            ACTIVATOR,
            INFO,
            "Arena_Champion",
            &[ask("T_HELLO"), say("T_TRIBUTE")],
        ),
        info(
            "K_TributeDone",
            "Chief",
            "",
            "Q_Tribute",
            DELIVERY,
            INFO,
            "",
            &[say("T_THANKS")],
        ),
        // Mira: a dialog tree; the quest only appears after asking for her story.
        info(
            "Mi_Talk",
            "Mira",
            "",
            "",
            GENERAL,
            PARENT,
            "",
            &[ask("T_HELLO")],
        ),
        info(
            "Mi_Story",
            "Mira",
            "Mi_Talk",
            "",
            GENERAL,
            INFO,
            "",
            &[ask("T_STORY"), say("T_STORY_A"), event("Mira_Story")],
        ),
        info(
            "Mi_Help",
            "Mira",
            "Mi_Talk",
            "Q_Mira",
            ACTIVATOR,
            INFO,
            "Mira_Story",
            &[ask("T_WORK"), say("T_MIRA")],
        ),
        info(
            "Mi_Done",
            "Mira",
            "",
            "Q_Mira",
            DELIVERY,
            INFO,
            "",
            &[say("T_THANKS")],
        ),
        // Alice closes everything.
        info(
            "A_Final",
            "Alice",
            "",
            "Q_Final",
            ACTIVATOR,
            INFO,
            "",
            &[ask("T_WORK"), say("T_FINAL")],
        ),
        info(
            "A_FinalDone",
            "Alice",
            "",
            "Q_Final",
            DELIVERY,
            INFO,
            "",
            &[say("T_THANKS")],
        ),
        // Noise the bot must not get lost in.
        info(
            "Mi_Weather",
            "Mira",
            "Mi_Talk",
            "",
            GENERAL,
            INFO,
            "",
            &[ask("T_WEATHER"), say("T_WEATHER_A")],
        ),
        info(
            "B_Smalltalk",
            "Bob",
            "",
            "",
            GENERAL,
            INFO,
            "",
            &[ask("T_WEATHER"), say("T_WEATHER_A")],
        ),
    ];
    (quests, infos)
}

fn texts() -> StringTable {
    strings(&[
        ("T_HELLO", "Hello.", "Hallo."),
        ("T_WORK", "Any work for me?", "Hast du Arbeit fuer mich?"),
        ("T_GIVE", "Here you are.", "Hier, bitte."),
        ("T_THANKS", "Thank you!", "Danke!"),
        ("T_TROUBLE", "We are in trouble.", "Wir haben Aerger."),
        (
            "T_LETTER",
            "Take this letter to Bob.",
            "Bring diesen Brief zu Bob.",
        ),
        (
            "T_HERBS",
            "Bring me three herbs.",
            "Bring mir drei Kraeuter.",
        ),
        ("T_WOLVES", "Kill two wolves.", "Toete zwei Woelfe."),
        (
            "T_ARENA",
            "Beat Brutus in the arena.",
            "Besiege Brutus in der Arena.",
        ),
        ("T_SCOUT", "Scout the pass.", "Erkunde den Pass."),
        (
            "T_ESCORT",
            "Take me to the captain.",
            "Bring mich zum Hauptmann.",
        ),
        ("T_RING", "I need a ring.", "Ich brauche einen Ring."),
        (
            "T_TRIBUTE",
            "Pay tribute to the champion.",
            "Ehre dem Champion.",
        ),
        ("T_STORY", "Tell me about yourself.", "Erzaehl von dir."),
        (
            "T_STORY_A",
            "My brother is lost.",
            "Mein Bruder ist verschollen.",
        ),
        ("T_MIRA", "Help me find him.", "Hilf mir, ihn zu finden."),
        ("T_FINAL", "You helped everyone.", "Du hast allen geholfen."),
        ("T_WEATHER", "Nice weather.", "Schoenes Wetter."),
        ("T_WEATHER_A", "Indeed.", "In der Tat."),
    ])
}

/// The ten-quest fixture.
pub fn database() -> GameDatabase {
    let (quests, infos) = records();
    GameDatabase::from_parts(quests, infos, texts(), "English").expect("fixture database")
}

/// The fixture plus two quests that cannot be done: `Q_Lost` needs an event nobody sets, and
/// `Q_Locked` needs `Q_Lost` finished.
pub fn broken() -> GameDatabase {
    use cond::*;
    use quest_type::*;
    use ty::*;
    let (mut quests, mut infos) = records();
    quests.push(quest("Q_Lost", REPORT, "", "Hermit", "1", ""));
    quests.push(quest("Q_Locked", REPORT, "Q_Lost", "Hermit", "1", ""));
    infos.push(info(
        "H_Lost",
        "Hermit",
        "",
        "Q_Lost",
        ACTIVATOR,
        INFO,
        "Never_Set",
        &[ask("T_WORK")],
    ));
    infos.push(info(
        "H_LostDone",
        "Hermit",
        "",
        "Q_Lost",
        DELIVERY,
        INFO,
        "",
        &[say("T_THANKS")],
    ));
    infos.push(info(
        "H_Locked",
        "Hermit",
        "",
        "Q_Locked",
        ACTIVATOR,
        INFO,
        "",
        &[ask("T_WORK")],
    ));
    GameDatabase::from_parts(quests, infos, texts(), "English").expect("broken fixture")
}

/// The fixture plus an NPC whose only line is a `Permanent` info belonging to `Q_Final`:
/// always available, it outscores every unlocking or exploring line, and choosing it never
/// changes anything. Real data has many permanent quest lines like it.
pub fn with_refusal() -> GameDatabase {
    use cond::*;
    use ty::*;
    let (quests, mut infos) = records();
    let mut not_yet = info(
        "G_NotYet",
        "Grump",
        "",
        "Q_Final",
        GENERAL,
        INFO,
        "",
        &[ask("T_WORK"), say("T_TROUBLE")],
    );
    not_yet.permanent = true;
    infos.push(not_yet);
    GameDatabase::from_parts(quests, infos, texts(), "English").expect("refusal fixture")
}

/// The fixture plus `Q_Gated`, whose activator needs `Gate_Open`. The only info setting that
/// event (`X_Gate` at Xan) itself needs an event nobody sets, so the stuck report has to name
/// the setter and why it cannot run.
pub fn gated() -> GameDatabase {
    use cond::*;
    use quest_type::*;
    use ty::*;
    let (mut quests, mut infos) = records();
    quests.push(quest("Q_Gated", REPORT, "", "Xan", "1", ""));
    infos.push(info(
        "X_Gate",
        "Xan",
        "",
        "",
        GENERAL,
        INFO,
        "Never_Set",
        &[ask("T_HELLO"), event("Gate_Open")],
    ));
    infos.push(info(
        "X_Gated",
        "Xan",
        "",
        "Q_Gated",
        ACTIVATOR,
        INFO,
        "Gate_Open",
        &[ask("T_WORK")],
    ));
    GameDatabase::from_parts(quests, infos, texts(), "English").expect("gated fixture")
}
