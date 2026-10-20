//! Input handling and the dialog driver (the only place that mutates the `GameSession`).
use crate::*;
use bevy::window::PrimaryWindow;
use gothic_gameplay::Engine;
use gothic_gameplay::model::QuestStatus;
use gothic_gameplay::script::ExecResult;
use gothic_gameplay::transcript::quest_title;

pub(crate) fn setup(
    mut commands: Commands,
    cfg: Res<UiConfig>,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(assets::load_install_assets(
        &cfg.install_dir,
        &mut fonts,
        &mut images,
    ));
    if cfg.spawn_camera {
        commands.spawn((Camera2d, IsDefaultUiCamera));
    }
}

pub(crate) fn ui_scale(
    cfg: Res<UiConfig>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut scale: ResMut<UiScale>,
) {
    if !cfg.auto_scale {
        return;
    }
    if let Ok(w) = windows.single() {
        let s = (w.width() / 1024.0).min(w.height() / 768.0).max(0.5);
        if (scale.0 - s).abs() > 0.001 {
            scale.0 = s;
        }
    }
}

const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];

pub(crate) fn hotkeys(
    cfg: Res<UiConfig>,
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<UiState>,
    mut act: MessageWriter<Act>,
) {
    if !cfg.hotkeys {
        return;
    }
    if ui.screen == Screen::Dialog {
        let n = ui.dialog.as_ref().map_or(0, |d| d.menu.len());
        for (i, k) in DIGITS.iter().enumerate() {
            if i < n && keys.just_pressed(*k) {
                act.write(Act(Action::Choose(i)));
            }
        }
        if n == 0 && keys.any_just_pressed([KeyCode::Space, KeyCode::Enter, KeyCode::Escape]) {
            act.write(Act(Action::Close));
        }
        return;
    }
    let toggles = [
        (KeyCode::KeyL, Screen::Journal),
        (KeyCode::KeyI, Screen::Inventory),
        (KeyCode::KeyC, Screen::Character),
        (KeyCode::KeyT, Screen::Trade),
    ];
    for (k, s) in toggles {
        if keys.just_pressed(k) {
            act.write(Act(if ui.screen == s {
                Action::Close
            } else {
                Action::Open(s)
            }));
        }
    }
    if keys.just_pressed(KeyCode::Escape) && ui.is_open() {
        act.write(Act(Action::Close));
    }
}

pub(crate) fn dispatch_actions(
    mut acts: MessageReader<Act>,
    mut ui: ResMut<UiState>,
    mut choose: MessageWriter<ChooseOption>,
    mut use_item: MessageWriter<UseItem>,
    mut trade: MessageWriter<TradeRequest>,
    mut ended: MessageWriter<DialogEnded>,
) {
    for Act(a) in acts.read() {
        match a.clone() {
            Action::Choose(index) => {
                choose.write(ChooseOption { index });
            }
            Action::Close => {
                if let Some(d) = ui.dialog.take() {
                    ended.write(DialogEnded {
                        npc: d.session.npc.clone(),
                    });
                }
                ui.set_screen(Screen::None);
            }
            Action::Open(s) => {
                let ok = match s {
                    Screen::Trade => ui.trade.is_some(),
                    Screen::Dialog => ui.dialog.is_some(),
                    _ => true,
                };
                if ok {
                    ui.set_screen(s);
                }
            }
            Action::InvTab(i) => {
                ui.inv_tab = i;
                ui.inv_sel = None;
                ui.dirty = true;
            }
            Action::QuestTab(i) => {
                ui.quest_tab = i;
                ui.quest_sel = None;
                ui.dirty = true;
            }
            Action::SelectQuest(q) => {
                ui.quest_sel = Some(q);
                ui.dirty = true;
            }
            Action::SelectItem(i) => {
                ui.inv_sel = Some(i);
                ui.dirty = true;
            }
            Action::UseItem(item) => {
                use_item.write(UseItem { item });
            }
            Action::Trade(item, buy) => {
                trade.write(TradeRequest { item, buy });
            }
        }
    }
}

fn npc_label(npc: &str) -> String {
    npc.replace('_', " ")
}

fn absorb(
    db: &gothic_gameplay::GameDatabase,
    npc: &str,
    r: &ExecResult,
    view: &mut DialogView,
    notify: &mut MessageWriter<Notify>,
    fx: &mut MessageWriter<WorldEffect>,
) {
    let mut toast = |text: String, kind| {
        notify.write(Notify { text, kind });
    };
    for o in &r.outputs {
        match o {
            Output::Say {
                speaker, text, key, ..
            } => {
                let player = speaker == "player";
                let who = if player {
                    "Du".to_string()
                } else if speaker.is_empty() || speaker.eq_ignore_ascii_case(npc) {
                    npc_label(npc)
                } else {
                    npc_label(speaker)
                };
                view.lines.push(DialogLine {
                    speaker: who,
                    text: text.clone().unwrap_or_else(|| format!("[{key}]")),
                    player,
                });
            }
            Output::Presentation { text: Some(t), .. } => view.lines.push(DialogLine {
                speaker: String::new(),
                text: t.clone(),
                player: false,
            }),
            Output::QuestChanged { quest, to, .. } => {
                let verb = match to {
                    QuestStatus::Running => "Neue Aufgabe",
                    QuestStatus::Success => "Aufgabe erfüllt",
                    QuestStatus::Failed => "Aufgabe gescheitert",
                    _ => continue,
                };
                toast(
                    format!("{verb}: {}", quest_title(db, quest)),
                    ToastKind::Quest,
                );
            }
            Output::QuestLog { quest, .. } => toast(
                format!("Tagebuch: {}", quest_title(db, quest)),
                ToastKind::Quest,
            ),
            Output::Xp {
                amount,
                level_before,
                level_after,
            } => {
                toast(format!("+{amount} EP"), ToastKind::Xp);
                if level_after > level_before {
                    toast(format!("Stufe {level_after} erreicht!"), ToastKind::Xp);
                }
            }
            Output::Item {
                from,
                to,
                item,
                amount,
            } => {
                let name = style::item_name(db, item);
                if to == "player" {
                    toast(format!("+{amount} {name}"), ToastKind::Item);
                } else if from == "player" {
                    toast(format!("-{amount} {name}"), ToastKind::Item);
                }
            }
            Output::Message { text } => toast(text.clone(), ToastKind::Info),
            Output::Effect { .. } => {
                fx.write(WorldEffect {
                    npc: npc.to_string(),
                    output: o.clone(),
                });
            }
            _ => {}
        }
    }
    if let Some(why) = &r.refused {
        toast(format!("Nicht möglich: {why}"), ToastKind::Info);
    }
    let n = view.lines.len();
    if n > 60 {
        view.lines.drain(..n - 60);
    }
}

pub(crate) fn drive_dialog(
    cfg: Res<UiConfig>,
    mut session: ResMut<GameSession>,
    mut ui: ResMut<UiState>,
    mut starts: MessageReader<StartDialog>,
    mut picks: MessageReader<ChooseOption>,
    mut notify: MessageWriter<Notify>,
    mut fx: MessageWriter<WorldEffect>,
) {
    if !cfg.drive_dialog {
        starts.clear();
        picks.clear();
        return;
    }
    let s = &mut *session;
    let eng = Engine::with(&s.db, s.assume);
    for st in starts.read() {
        let (sess, r) = eng.start(&mut s.state, &st.npc);
        let mut view = DialogView {
            session: sess,
            lines: Vec::new(),
            menu: Vec::new(),
        };
        absorb(&s.db, &st.npc, &r, &mut view, &mut notify, &mut fx);
        view.menu = eng.menu(&s.state, &view.session);
        ui.dialog = Some(view);
        ui.set_screen(Screen::Dialog);
    }
    for p in picks.read() {
        let Some(view) = ui.dialog.as_mut() else {
            continue;
        };
        let Some(c) = view.menu.get(p.index).cloned() else {
            continue;
        };
        let npc = view.session.npc.clone();
        let r = eng.choose(&mut s.state, &mut view.session, &c);
        absorb(&s.db, &npc, &r, view, &mut notify, &mut fx);
        view.menu = eng.menu(&s.state, &view.session);
        ui.dirty = true;
    }
}

pub(crate) fn screens_requests(
    mut open: MessageReader<OpenScreen>,
    mut close: MessageReader<CloseScreen>,
    mut trades: MessageReader<OpenTrade>,
    mut act: MessageWriter<Act>,
    mut ui: ResMut<UiState>,
) {
    for t in trades.read() {
        ui.trade = Some((t.npc.clone(), t.stock.clone()));
        ui.set_screen(Screen::Trade);
    }
    for o in open.read() {
        act.write(Act(Action::Open(o.0)));
    }
    for _ in close.read() {
        act.write(Act(Action::Close));
    }
}

/// Show / update / close a dialog whose conversation is run by the host ([`DialogUpdate`]).
pub(crate) fn apply_dialog_updates(
    mut updates: MessageReader<DialogUpdate>,
    mut ui: ResMut<UiState>,
) {
    for u in updates.read() {
        if !u.open {
            ui.dialog = None;
            if ui.screen == Screen::Dialog {
                ui.set_screen(Screen::None);
            }
            continue;
        }
        let fresh = ui
            .dialog
            .as_ref()
            .is_none_or(|d| d.session.npc != u.session.npc);
        if fresh {
            ui.dialog = Some(DialogView {
                session: u.session.clone(),
                lines: Vec::new(),
                menu: Vec::new(),
            });
        }
        if let Some(view) = ui.dialog.as_mut() {
            view.session = u.session.clone();
            for (speaker, text, player) in &u.lines {
                view.lines.push(DialogLine {
                    speaker: speaker.clone(),
                    text: text.clone(),
                    player: *player,
                });
            }
            let n = view.lines.len();
            if n > 60 {
                view.lines.drain(..n - 60);
            }
            view.menu = u.menu.clone();
        }
        ui.set_screen(Screen::Dialog);
        ui.dirty = true;
    }
}
