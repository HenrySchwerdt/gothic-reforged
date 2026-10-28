//! Replay a dialog by info names and print the quest statuses after each step (debug aid):
//! `replay <Data> <quest> <npc> <info> [<info> ...]`.
use gothic_gameplay::{Assumptions, Engine, GameDatabase, GameState};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let db = GameDatabase::load_install(Path::new(&a[1]), "German").unwrap();
    let mut st = GameState::new();
    let eng = Engine::with(&db, Assumptions::PERMISSIVE);
    let (mut s, r) = eng.start(&mut st, &a[3]);
    println!("start: {:?} status {:?}", r.outputs.len(), st.quest_status(&a[2]));
    for want in &a[4..] {
        let menu = eng.menu(&st, &s);
        println!("  menu: {:?}", menu.iter().map(|c| (c.info.map(|i| db.info(i).name.clone()), c.label.chars().take(30).collect::<String>())).collect::<Vec<_>>());
        let pick = menu.iter().find(|c| {
            c.info
                .is_some_and(|i| db.info(i).name.eq_ignore_ascii_case(want))
        });
        let Some(c) = pick.cloned() else {
            println!("{want}: not in the menu {:?}", menu.iter().map(|c| c.info.map(|i| db.info(i).name.clone())).collect::<Vec<_>>());
            return;
        };
        let r = eng.choose(&mut st, &mut s, &c);
        println!(
            "{want}: quest {:?}; outputs {:?}",
            st.quest_status(&a[2]),
            r.outputs
                .iter()
                .filter_map(|o| match o {
                    gothic_gameplay::script::Output::QuestChanged { quest, to, .. } => Some(format!("{quest}->{to:?}")),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
    }
}
