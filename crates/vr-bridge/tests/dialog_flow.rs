//! The VR dialogue follows the original flow: lines in order, options only after the last line,
//! `Choose` for a pick, close after pending lines.
use gothic_gameplay::dialog::Choice;
use gothic_runtime::Command;
use gothic_sim::types::ActorId;
use gothic_vr_bridge::dialog_flow::{DialogFlow, FlowOut, apply_to_hud};
use gothic_vr_ui::HudData;

fn menu() -> Vec<Choice> {
    ["Wer bist du?", "Ich suche Arbeit.", "Ich muss weiter."]
        .iter()
        .map(|l| Choice { info: None, label: (*l).into() })
        .collect()
}

#[test]
fn options_wait_for_the_last_line_and_a_choice_sends_choose() {
    let mut f = DialogFlow::default();
    f.on_started(ActorId(3));
    f.on_line("Hamlar", "Hello_01", Some("Was willst du, Fremder?"), false, Some(ActorId(3)));
    f.on_line("Hamlar", "Hello_02", Some("Ich habe zu tun."), false, Some(ActorId(3)));
    f.on_menu(&menu());
    // First tick: the first line, no options yet.
    let o = f.tick(0.016);
    assert!(matches!(&o[0], FlowOut::Say { line, .. } if line.text.starts_with("Was willst")));
    assert_eq!(o.len(), 1, "{o:?}");
    assert!(f.choose(0).is_none(), "cannot pick while the NPC speaks");
    // Skipping (trigger) moves to the second line, then the options.
    f.finish_line();
    let o = f.tick(0.016);
    assert!(matches!(&o[0], FlowOut::Say { line, .. } if line.voice.as_deref() == Some("Hello_02")));
    f.finish_line();
    let o = f.tick(0.016);
    assert_eq!(o, vec![FlowOut::ShowOptions(vec!["Wer bist du?".into(), "Ich suche Arbeit.".into(), "Ich muss weiter.".into()])]);
    let (cmd, out) = f.choose(1).expect("pick");
    assert!(matches!(cmd, Command::Choose { index: 1 }));
    assert_eq!(out, FlowOut::HideOptions);
    assert!(f.choose(1).is_none(), "options are gone until the next menu");
}

#[test]
fn lines_time_out_by_reading_time_and_the_dialogue_closes_after_pending_lines() {
    let mut f = DialogFlow::default();
    f.on_started(ActorId(1));
    f.on_line("Du", "", Some("Auf Wiedersehen."), true, None);
    f.on_ended();
    let o = f.tick(0.0);
    assert!(matches!(&o[0], FlowOut::Say { line, secs } if line.player && *secs >= 2.0));
    assert!(f.tick(1.0).is_empty(), "still reading");
    let o = f.tick(3.0);
    assert_eq!(o, vec![FlowOut::Closed]);
    assert!(!f.is_open());
}

#[test]
fn hud_gets_the_subtitle_at_the_speaker_and_the_options_at_the_npc() {
    let mut f = DialogFlow::default();
    let mut data = HudData::default();
    let npc_head = glam::Vec3::new(0.0, 1.7, -2.0);
    let heads = |a: Option<ActorId>| if a.is_some() { npc_head } else { glam::Vec3::new(0.0, 1.7, 0.0) };
    f.on_started(ActorId(2));
    f.on_line("Hamlar", "k", Some("Guten Tag."), false, Some(ActorId(2)));
    f.on_menu(&menu());
    for o in f.tick(0.0) {
        apply_to_hud(&o, &mut data, npc_head, &heads);
    }
    assert_eq!(data.speaker_head, Some(npc_head));
    assert_eq!(data.subtitle.text, "Guten Tag.");
    assert!(data.dialog.is_none());
    f.finish_line();
    for o in f.tick(0.0) {
        apply_to_hud(&o, &mut data, npc_head, &heads);
    }
    assert_eq!(data.dialog.as_ref().unwrap().options.len(), 3);
}
