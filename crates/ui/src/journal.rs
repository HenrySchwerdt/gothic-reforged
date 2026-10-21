//! View models of the quest log and the skills page: tab assignment and ordering of quests,
//! the day / time widget, and what a teacher offers (learning points and gold).
//!
//! Ported from `gCHUDViewQuests2` (time widget, Game:0x10084690), `gCHUDViewSkills2` and the
//! teach rules of `gCInfoManager_PS`. UI code reads these and never mutates game state.
use gothic_gameplay::GameDatabase;
use gothic_gameplay::GameState;
use gothic_gameplay::model::{InfoCond, QuestStatus};
use gothic_gameplay::state::LogKind;

/// Quest tab of a status: 0 = running, 1 = succeeded, 2 = failed / cancelled. Quests that
/// have not started (`Open`) are not listed.
pub fn quest_tab(s: QuestStatus) -> Option<usize> {
    match s {
        QuestStatus::Running => Some(0),
        QuestStatus::Success | QuestStatus::Won => Some(1),
        QuestStatus::Failed
        | QuestStatus::Lost
        | QuestStatus::Obsolete
        | QuestStatus::Cancelled => Some(2),
        QuestStatus::Open => None,
    }
}

/// One entry of the quest list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestRow {
    pub name: String,
    pub title: String,
    /// Index of the newest log entry of the quest (0 = none).
    pub updated: usize,
}

/// Quests of a tab, most recently updated first, then by title. The original orders its list
/// control by insertion; recency-first is our choice so new tasks surface (see PORT_NOTES).
pub fn quest_rows(state: &GameState, tab: usize, title: impl Fn(&str) -> String) -> Vec<QuestRow> {
    let mut rows: Vec<QuestRow> = state
        .quests
        .iter()
        .filter(|(_, q)| q.status.and_then(quest_tab) == Some(tab))
        .map(|(name, _)| QuestRow {
            name: name.clone(),
            title: title(name),
            updated: state
                .log
                .iter()
                .rposition(|e| &e.quest == name)
                .map_or(0, |i| i + 1),
        })
        .collect();
    rows.sort_by(|a, b| {
        b.updated
            .cmp(&a.updated)
            .then_with(|| a.title.cmp(&b.title))
    });
    rows
}

/// Status line of a quest log entry ("Aufgabe begonnen"), `None` for statuses without one.
pub fn status_line(kind: &LogKind) -> Option<&'static str> {
    match kind {
        LogKind::Status(QuestStatus::Running) => Some("Aufgabe begonnen"),
        LogKind::Status(QuestStatus::Success | QuestStatus::Won) => Some("Aufgabe erfüllt"),
        LogKind::Status(QuestStatus::Failed) => Some("Aufgabe gescheitert"),
        _ => None,
    }
}

/// Which of the four time-of-day pictures of the quest view to show for `hour`
/// (0 dawn, 1 day, 2 dusk, 3 night). Port of the selection in Game:0x10084690.
pub fn time_of_day(hour: u32) -> usize {
    match hour {
        0..=5 => 3,
        6..=7 => 0,
        8..=19 => 1,
        20..=21 => 2,
        _ => 3,
    }
}

/// Day (1-based) and hour/minute of the world clock from elapsed game milliseconds.
pub fn clock_from_ms(game_ms: u64) -> (u64, u32, u32) {
    let day = game_ms / 86_400_000;
    let rem = game_ms % 86_400_000;
    (
        day + 1,
        (rem / 3_600_000) as u32,
        (rem / 60_000 % 60) as u32,
    )
}

/// Fill the `$(day)`, `$(hour)`, `$(minute)`, `$(year)` placeholders of the localized clock
/// format; formats shorter than six characters (an unset string) fall back to the original's
/// built-in `"$(day). day. $(hour):$(minute)"`. Hour and minute are zero padded to two digits.
pub fn format_clock(template: &str, year: u64, day: u64, hour: u32, minute: u32) -> String {
    let t = if template.chars().count() < 6 {
        "$(day). day. $(hour):$(minute)"
    } else {
        template
    };
    t.replace("$(year)", &year.to_string())
        .replace("$(day)", &day.to_string())
        .replace("$(hour)", &format!("{hour:02}"))
        .replace("$(minute)", &format!("{minute:02}"))
        .replace("\\n", ", ")
        .replace('\n', ", ")
}

/// One thing a teacher offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeachOffer {
    pub info: String,
    /// Attribute tag or skill key.
    pub subject: String,
    pub is_skill: bool,
    /// Attribute points taught (0 for skills).
    pub points: u32,
    pub lp_cost: u32,
    pub gold_cost: u32,
    pub learned: bool,
    pub affordable: bool,
}

/// Rule that turns attribute points into a learning-point cost
/// (`gothic_sim::economy::teach_cost` in the game; 1:1 by default).
pub type CostRule = dyn Fn(&str, i32, u32) -> u32 + Send + Sync;

/// Port of the teach offers of a teacher NPC: every `Teach` info of the owner that the
/// player has not been given yet, with its learning-point and gold price. Skill LP costs live
/// in property sets that are not decoded yet, so skills cost 0 LP here (as `script::teach`).
pub fn teach_offers(
    db: &GameDatabase,
    state: &GameState,
    npc: &str,
    cost: &CostRule,
) -> Vec<TeachOffer> {
    let gold = state.inventory.count(gothic_gameplay::state::GOLD_ITEM);
    let mut out = Vec::new();
    for &id in db.infos_of(npc) {
        let info = db.info(id);
        if InfoCond::from_raw(info.condition_type) != InfoCond::Teach {
            continue;
        }
        let is_skill = info.teach_attrib.is_empty() && !info.teach_skill.is_empty();
        let is_attr = !info.teach_attrib.is_empty() && info.teach_attrib_value > 0;
        if !is_skill && !is_attr {
            continue;
        }
        let subject = if is_skill {
            info.teach_skill.to_ascii_lowercase()
        } else {
            info.teach_attrib.to_ascii_lowercase()
        };
        let current = state.attributes.get(&subject).copied().unwrap_or(0);
        let lp_cost = if is_attr {
            cost(&subject, current, info.teach_attrib_value)
        } else {
            0
        };
        let learned = is_skill && state.skills.contains(&subject);
        out.push(TeachOffer {
            info: info.name.clone(),
            points: if is_attr { info.teach_attrib_value } else { 0 },
            is_skill,
            lp_cost,
            gold_cost: info.gold_cost,
            learned,
            affordable: !learned
                && state.learning_points >= lp_cost
                && i64::from(gold) >= i64::from(info.gold_cost),
            subject,
        });
    }
    out.sort_by(|a, b| a.subject.cmp(&b.subject).then(a.points.cmp(&b.points)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_gameplay::state::{LogEntry, QuestRuntime};

    fn quest(state: &mut GameState, name: &str, s: QuestStatus) {
        let q = QuestRuntime {
            status: Some(s),
            ..QuestRuntime::default()
        };
        state.quests.insert(name.to_string(), q);
    }

    #[test]
    fn statuses_map_to_the_three_tabs() {
        assert_eq!(quest_tab(QuestStatus::Running), Some(0));
        assert_eq!(quest_tab(QuestStatus::Won), Some(1));
        assert_eq!(quest_tab(QuestStatus::Cancelled), Some(2));
        assert_eq!(quest_tab(QuestStatus::Open), None);
    }

    #[test]
    fn quests_sort_by_recent_update_then_title() {
        let mut st = GameState::new();
        quest(&mut st, "b", QuestStatus::Running);
        quest(&mut st, "a", QuestStatus::Running);
        quest(&mut st, "c", QuestStatus::Running);
        quest(&mut st, "done", QuestStatus::Success);
        for q in ["a", "b", "a"] {
            st.log.push(LogEntry {
                quest: q.into(),
                kind: LogKind::Text("t".into()),
            });
        }
        let rows = quest_rows(&st, 0, |n| n.to_uppercase());
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        assert_eq!(rows[0].title, "A");
        assert_eq!(quest_rows(&st, 1, |n| n.into()).len(), 1);
        assert!(quest_rows(&st, 2, |n| n.into()).is_empty());
    }

    #[test]
    fn time_of_day_boundaries_follow_the_original() {
        let t: Vec<_> = [0, 5, 6, 7, 8, 19, 20, 21, 22, 23]
            .map(time_of_day)
            .to_vec();
        assert_eq!(t, [3, 3, 0, 0, 1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn clock_splits_game_milliseconds() {
        assert_eq!(clock_from_ms(0), (1, 0, 0));
        assert_eq!(
            clock_from_ms(86_400_000 + 13 * 3_600_000 + 7 * 60_000),
            (2, 13, 7)
        );
    }

    #[test]
    fn clock_template_fills_and_falls_back() {
        assert_eq!(
            format_clock("$(day). Tag, $(hour):$(minute)", 1, 3, 9, 5),
            "3. Tag, 09:05"
        );
        assert_eq!(format_clock("", 1, 3, 9, 5), "3. day. 09:05");
        assert_eq!(format_clock("$(day)\\n$(hour)h", 1, 4, 22, 0), "4, 22h");
    }

    #[test]
    fn status_lines_cover_the_three_outcomes() {
        assert_eq!(
            status_line(&LogKind::Status(QuestStatus::Won)),
            Some("Aufgabe erfüllt")
        );
        assert_eq!(status_line(&LogKind::Status(QuestStatus::Cancelled)), None);
        assert_eq!(status_line(&LogKind::Text("x".into())), None);
    }

    fn teach_db() -> GameDatabase {
        let mut raw = Vec::new();
        for (name, extra) in [
            (
                "T_STR",
                "TeachAttrib=STR\r\nTeachAttribValue=5\r\nGoldCost=100\r\n",
            ),
            ("T_SKILL", "TeachSkill=Perk_Sneak\r\n"),
            ("T_NONE", ""),
        ] {
            raw.push(format!(
                "[Info]\r\nName={name}\r\nSortID=1\r\nOwner=Teacher\r\nInfoGiven=false\r\nParent=\r\nQuest=\r\nConditionType=16\r\nInfoType=3\r\n{extra}"
            ));
        }
        let infos = raw
            .iter()
            .map(|t| gothic_formats::info::Info::parse(t.as_bytes()).unwrap())
            .collect();
        let text = "[LocAdmin_Languages]\r\nLanguages=German\r\nCurrentLanguage=German\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n[LocAdmin_Revisions]\r\n";
        let mut b = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        let strings = gothic_formats::stringtable::StringTable::parse(&b).unwrap();
        GameDatabase::from_parts(Vec::new(), infos, strings, "German").unwrap()
    }

    #[test]
    fn teacher_offers_carry_lp_and_gold_prices() {
        let db = teach_db();
        let mut st = GameState::new();
        st.learning_points = 5;
        let double = |_: &str, _: i32, p: u32| p * 2;
        let offers = teach_offers(&db, &st, "Teacher", &double);
        assert_eq!(offers.len(), 2);
        let attr = offers.iter().find(|o| !o.is_skill).unwrap();
        assert_eq!(
            (attr.subject.as_str(), attr.points, attr.lp_cost),
            ("str", 5, 10)
        );
        assert!(!attr.affordable, "10 LP needed, 5 held, no gold");
        let skill = offers.iter().find(|o| o.is_skill).unwrap();
        assert_eq!(skill.subject, "perk_sneak");
        assert!(skill.affordable && !skill.learned);
        st.skills.insert("perk_sneak".into());
        let again = teach_offers(&db, &st, "Teacher", &|_, _, p| p);
        assert!(again.iter().find(|o| o.is_skill).unwrap().learned);
    }
}
