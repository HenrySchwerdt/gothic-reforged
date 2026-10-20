use gothic_gameplay::db::GameDatabase;
use std::collections::BTreeMap;
fn main() {
    let db = GameDatabase::load_install(
        std::path::Path::new("D:/SteamLibrary/steamapps/common/Gothic 3/Data"),
        "German",
    )
    .unwrap();
    #[derive(Default)]
    struct S {
        n: usize,
        quest: usize,
        parent: usize,
        knows: usize,
        items: usize,
        gold: usize,
        sec: usize,
        perm: usize,
        clr: usize,
        types: BTreeMap<u32, usize>,
        cmds: BTreeMap<String, usize>,
    }
    let mut m: BTreeMap<u32, S> = BTreeMap::new();
    for i in &db.infos {
        let s = m.entry(i.condition_type).or_default();
        s.n += 1;
        s.quest += !i.quest.is_empty() as usize;
        s.parent += !i.parent.is_empty() as usize;
        s.knows += !i.cond_player_knows.is_empty() as usize;
        s.items += !i.cond_items.is_empty() as usize;
        s.gold += (i.gold_cost > 0) as usize;
        s.sec += !i.cond_secondary_npc.is_empty() as usize;
        s.perm += i.permanent as usize;
        s.clr += i.clear_children as usize;
        *s.types.entry(i.info_type).or_default() += 1;
        for st in &i.script {
            if st.command != "Say" && st.command != "Description" {
                *s.cmds.entry(st.command.clone()).or_default() += 1;
            }
        }
    }
    let mut x: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for i in &db.infos {
        if let Some(q) = db.quest_by_name(&i.quest) {
            *x.entry((q.quest_type, i.condition_type)).or_default() += 1;
        }
    }
    let mut cur = 999;
    for ((t, c), n) in &x {
        if *t != cur {
            println!();
            print!("type {t}:");
            cur = *t;
        }
        print!(" ct{c}={n}");
    }
    println!();
    let mut h: BTreeMap<String, usize> = BTreeMap::new();
    for i in &db.infos {
        for s in &i.cond_secondary_npc_states {
            *h.entry(format!("secstate {s}")).or_default() += 1;
        }
        for s in &i.cond_item_container {
            *h.entry(format!("container {s}")).or_default() += 1;
        }
        if !i.cond_owner_near_entity.is_empty() {
            *h.entry(format!("near {}", i.cond_owner_near_entity))
                .or_default() += 1;
        }
        if !i.cond_player_party_member.is_empty() {
            *h.entry(format!("party {}", i.cond_player_party_member))
                .or_default() += 1;
        }
        if i.cond_items.len() != i.cond_item_amounts.len()
            || (!i.cond_item_container.is_empty()
                && i.cond_item_container.len() != i.cond_items.len())
        {
            *h.entry("items len mismatch".into()).or_default() += 1;
        }
        if i.cond_secondary_npc.len() != i.cond_secondary_npc_states.len() {
            *h.entry("sec len mismatch".into()).or_default() += 1;
        }
        for (k, v) in &i.extra {
            *h.entry(format!("extra {k}={v}")).or_default() += 1;
        }
        *h.entry(format!("infotype {} ct {}", i.info_type, i.condition_type))
            .or_default() += 0;
    }
    for (k, v) in h.iter().filter(|x| *x.1 > 0) {
        println!("{k}: {v}");
    }
    if let Ok(w) = std::env::var("SEC") {
        let w: u32 = w.parse().unwrap();
        let mut n = 0;
        for i in &db.infos {
            if i.cond_secondary_npc_states.contains(&w) && n < 8 {
                n += 1;
                let t = i
                    .script
                    .iter()
                    .find(|s| s.command == "Say")
                    .and_then(|s| db.text(&s.text))
                    .unwrap_or_default();
                println!(
                    "SEC{w} {} owner={} ct={} sec={:?}/{:?} knows={:?} :: {}",
                    i.name,
                    i.owner,
                    i.condition_type,
                    i.cond_secondary_npc,
                    i.cond_secondary_npc_states,
                    i.cond_player_knows,
                    t.chars().take(80).collect::<String>()
                );
            }
        }
    }
    {
        let mut h: BTreeMap<String, usize> = BTreeMap::new();
        let mut shown = 0;
        for i in &db.infos {
            for st in &i.script {
                if matches!(
                    st.command.as_str(),
                    "CloseQuest" | "RunQuest" | "SucceedQuest" | "FailQuest" | "AddQuestLog"
                ) {
                    let same = st.id1.eq_ignore_ascii_case(&i.quest);
                    *h.entry(format!(
                        "{} ct={} same_quest={}",
                        st.command, i.condition_type, same
                    ))
                    .or_default() += 1;
                }
                if st.command == "Description" && shown < 6 {
                    shown += 1;
                    println!(
                        "DESC {} e1={} id1={} id2={} text={} -> {:?}",
                        i.name,
                        st.entity1,
                        st.id1,
                        st.id2,
                        st.text,
                        db.text(&st.text)
                            .map(|t| t.chars().take(100).collect::<String>())
                    );
                }
                if st.command == "AddQuestLog" {
                    println!("AQL {} {:?}", i.name, st);
                }
            }
        }
        for (k, v) in &h {
            println!("QC {k}: {v}");
        }
    }
    if let Ok(list) = std::env::var("CMDS") {
        for c in list.split(',') {
            let mut n = 0;
            for i in &db.infos {
                for st in &i.script {
                    if st.command == c && n < 4 {
                        n += 1;
                        println!(
                            "CMD {c} [{} ct={}] e1={} e2={} i1={} i2={} t={}",
                            i.name,
                            i.condition_type,
                            st.entity1,
                            st.entity2,
                            st.id1,
                            st.id2,
                            st.text
                        );
                    }
                }
            }
        }
    }
    // XP check
    let (mut same, mut diff, mut none) = (0, 0, 0);
    for q in &db.quests {
        let mut xp = 0u32;
        let mut has = false;
        for id in db.infos_of_quest(&q.name) {
            for st in &db.info(*id).script {
                if st.command == "GiveXP" {
                    has = true;
                    xp += st.id1.parse::<u32>().unwrap_or(0);
                }
            }
        }
        if !has {
            none += 1;
        } else if xp == q.experience_points {
            same += 1;
        } else {
            diff += 1;
            if diff < 6 {
                println!(
                    "xp {} quest={} giveXP sum={}",
                    q.name, q.experience_points, xp
                );
            }
        }
    }
    println!("xp same={same} diff={diff} nogive={none}");
    for (ct, s) in &m {
        let mut c: Vec<_> = s.cmds.iter().collect();
        c.sort_by_key(|x| std::cmp::Reverse(*x.1));
        let c: Vec<String> = c.iter().take(8).map(|(k, v)| format!("{k}:{v}")).collect();
        println!(
            "ct={ct} n={} quest={} parent={} knows={} items={} gold={} sec={} perm={} clr={} types={:?} cmds={}",
            s.n,
            s.quest,
            s.parent,
            s.knows,
            s.items,
            s.gold,
            s.sec,
            s.perm,
            s.clr,
            s.types,
            c.join(" ")
        );
    }
}

#[allow(dead_code)]
fn unused() {}
