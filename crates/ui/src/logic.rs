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
    if ui.modal.is_some() {
        if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
            act.write(Act(Action::Modal(true)));
        }
        if keys.just_pressed(KeyCode::Escape) {
            act.write(Act(Action::Modal(false)));
        }
        return;
    }
    if ui.page.is_some() && ui.screen != Screen::Document {
        // trade and loot pages: the host reads the keys, only Esc closes here
        if keys.just_pressed(KeyCode::Escape) {
            act.write(Act(Action::Escape));
        }
        return;
    }
    if ui.screen == Screen::Dialog {
        let n = ui.dialog.as_ref().map_or(0, |d| d.menu.len());
        for (i, k) in DIGITS.iter().enumerate() {
            if i < n && keys.just_pressed(*k) {
                act.write(Act(Action::Choose(i)));
            }
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            act.write(Act(Action::OptionStep(1)));
        }
        if keys.just_pressed(KeyCode::ArrowUp) {
            act.write(Act(Action::OptionStep(-1)));
        }
        if n > 0
            && keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space])
            && let Some(i) = ui.options.selected()
        {
            act.write(Act(Action::Choose(i)));
        }
        if n == 0 && keys.any_just_pressed([KeyCode::Space, KeyCode::Enter, KeyCode::Escape]) {
            act.write(Act(Action::Close));
        }
        return;
    }
    if matches!(ui.screen, Screen::MainMenu | Screen::Files | Screen::Video) {
        if keys.just_pressed(KeyCode::Escape) {
            act.write(Act(Action::Escape));
        }
        return;
    }
    if ui.screen == Screen::Document {
        if keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::PageDown]) {
            act.write(Act(Action::BookSpread(1)));
        }
        if keys.any_just_pressed([KeyCode::ArrowLeft, KeyCode::PageUp]) {
            act.write(Act(Action::BookSpread(-1)));
        }
        if keys.just_pressed(KeyCode::Escape) {
            act.write(Act(Action::Escape));
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
    if keys.just_pressed(KeyCode::Escape) {
        act.write(Act(Action::Escape));
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Outbox<'w> {
    choose: MessageWriter<'w, ChooseOption>,
    use_item: MessageWriter<'w, UseItem>,
    trade: MessageWriter<'w, TradeRequest>,
    pages: MessageWriter<'w, PageEvent>,
    ended: MessageWriter<'w, DialogEnded>,
    menu: MessageWriter<'w, MenuChoice>,
    answer: MessageWriter<'w, ModalAnswer>,
    files: MessageWriter<'w, FileCommand>,
    video: MessageWriter<'w, ApplyVideo>,
}

/// Close whatever is on show (page, dialog, screen).
fn close_all(ui: &mut UiState, out: &mut Outbox) {
    if ui.page.take().is_some() {
        out.pages.write(PageEvent::Close);
    }
    if let Some(d) = ui.dialog.take() {
        out.ended.write(DialogEnded {
            npc: d.session.npc.clone(),
        });
    }
    ui.set_screen(Screen::None);
}

/// Open the save / load page: the host answers with a [`SlotList`].
fn open_files(ui: &mut UiState, out: &mut Outbox, save_mode: bool) {
    ui.files = FilesState {
        save_mode,
        can_save: ui.menu_can_save,
        ..FilesState::default()
    };
    out.files.write(FileCommand::Refresh);
    ui.set_screen(Screen::Files);
}

fn open_video(ui: &mut UiState, current: &VideoOptions) {
    ui.video = current.clone();
    ui.set_screen(Screen::Video);
}

pub(crate) fn dispatch_actions(
    mut acts: MessageReader<Act>,
    mut ui: ResMut<UiState>,
    mut out: Outbox,
    gate: Res<HostGate>,
    mut video: ResMut<VideoOptions>,
) {
    for Act(a) in acts.read() {
        match a.clone() {
            Action::Choose(index) => {
                out.choose.write(ChooseOption { index });
            }
            Action::Close => close_all(&mut ui, &mut out),
            Action::Escape => {
                let modal = ui.modal.is_some();
                match ui.gui.escape(modal, gate.esc_blocked) {
                    EscOutcome::ClosedPage => close_all(&mut ui, &mut out),
                    EscOutcome::OpenedMenu | EscOutcome::MenuBack(_) => {
                        ui.set_screen(Screen::MainMenu);
                    }
                    EscOutcome::ClosedMenu => ui.set_screen(Screen::None),
                    EscOutcome::Modal | EscOutcome::Ignored => {}
                }
            }
            Action::MenuBack => {
                if ui.gui.is_menu_open() {
                    ui.set_screen(Screen::MainMenu);
                }
            }
            Action::Open(s) => {
                let ok = match s {
                    Screen::Trade => {
                        ui.trade.is_some() || matches!(ui.page, Some(PageView::Trade(_)))
                    }
                    Screen::Loot => matches!(ui.page, Some(PageView::Loot(_))),
                    Screen::Recipe => matches!(ui.page, Some(PageView::Recipe(_))),
                    Screen::Document => matches!(ui.page, Some(PageView::Document(_))),
                    Screen::Dialog => ui.dialog.is_some(),
                    Screen::Files => {
                        open_files(&mut ui, &mut out, false);
                        continue;
                    }
                    Screen::Video => {
                        open_video(&mut ui, &video);
                        continue;
                    }
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
                out.use_item.write(UseItem { item });
            }
            Action::Menu(e) => {
                out.menu.write(MenuChoice(e));
                match e {
                    MenuEntry::LoadGame => open_files(&mut ui, &mut out, false),
                    MenuEntry::SaveGame => open_files(&mut ui, &mut out, true),
                    MenuEntry::Options => open_video(&mut ui, &video),
                    _ => {}
                }
            }
            Action::TipCheck => {
                ui.tip_never = !ui.tip_never;
                ui.dirty = true;
            }
            Action::Modal(yes) => {
                if let Some(m) = ui.modal.take() {
                    match m {
                        Modal::Confirm { tag, .. } => {
                            if let Some(cmd) = files::confirm_command(&tag, yes) {
                                let leave = matches!(cmd, FileCommand::Save { .. });
                                out.files.write(cmd);
                                if leave {
                                    ui.set_screen(Screen::None);
                                }
                            }
                            out.answer.write(ModalAnswer { tag, yes });
                        }
                        Modal::Tip { .. } => {
                            out.answer.write(ModalAnswer {
                                tag: "tip".into(),
                                yes: ui.tip_never,
                            });
                        }
                    }
                    ui.dirty = true;
                }
            }
            Action::Trade(item, buy) => {
                out.trade.write(TradeRequest { item, buy });
            }
            Action::Page(e) => {
                out.pages.write(e);
            }
            Action::OptionStep(d) => {
                ui.options.step(d);
                ui.dirty = true;
            }
            Action::FileSelect(i) => {
                ui.files.selected = i;
                ui.dirty = true;
            }
            Action::FileDo => file_primary(&mut ui, &mut out),
            Action::FileDelete => {
                if let Some(row) = ui.files.selected.and_then(|i| ui.files.rows.get(i)) {
                    let key = row.key.clone();
                    ui.modal = Some(Modal::Confirm {
                        tag: format!("{}:{key}", files::CONFIRM_DELETE),
                        text: "Spielstand wirklich löschen?".into(),
                        yes: "Ja".into(),
                        no: "Nein".into(),
                    });
                    ui.dirty = true;
                }
            }
            Action::VideoMode(d) => {
                let modes = options::DEFAULT_MODES;
                let i = ui.video.mode_index(modes) as i32;
                let to = (i + d).clamp(0, modes.len() as i32 - 1) as usize;
                (ui.video.width, ui.video.height) = modes[to];
                ui.dirty = true;
            }
            Action::VideoSlider(slider, steps) => {
                let v = ui.video.slider(slider) + steps as f32 * 0.05;
                ui.video.set_slider(slider, v);
                ui.dirty = true;
            }
            Action::VideoVsync => {
                ui.video.vsync = !ui.video.vsync;
                ui.dirty = true;
            }
            Action::VideoApply => {
                *video = ui.video.clone();
                out.video.write(ApplyVideo(ui.video.clone()));
            }
            Action::SelectRecipe(i) => {
                ui.recipe_sel = i;
                ui.dirty = true;
            }
            Action::BookSpread(d) => {
                ui.spread = (ui.spread as i64 + i64::from(d)).max(0) as usize;
                ui.dirty = true;
            }
        }
    }
}

/// The page's main button: load the selected slot, or save (a new slot, or after the overwrite
/// confirmation).
fn file_primary(ui: &mut UiState, out: &mut Outbox) {
    let row = ui
        .files
        .selected
        .and_then(|i| ui.files.rows.get(i))
        .cloned();
    if !ui.files.save_mode {
        if let Some(r) = row {
            out.files.write(FileCommand::Load { key: r.key });
            ui.set_screen(Screen::None);
        }
        return;
    }
    if !ui.files.can_save {
        return;
    }
    match row {
        Some(r) => {
            ui.modal = Some(Modal::Confirm {
                tag: format!("{}:{}", files::CONFIRM_OVERWRITE, r.key),
                text: "Spielstand überschreiben?".into(),
                yes: "Ja".into(),
                no: "Nein".into(),
            });
            ui.dirty = true;
        }
        None => {
            out.files.write(FileCommand::Save { key: None });
            ui.set_screen(Screen::None);
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn screens_requests(
    mut open: MessageReader<OpenScreen>,
    mut close: MessageReader<CloseScreen>,
    mut trades: MessageReader<OpenTrade>,
    mut pages: MessageReader<ShowPage>,
    mut modals: MessageReader<ShowModal>,
    mut slots: MessageReader<SlotList>,
    mut act: MessageWriter<Act>,
    mut ui: ResMut<UiState>,
) {
    for l in slots.read() {
        ui.files.rows = l.rows.clone();
        ui.files.can_save = l.can_save;
        ui.files.selected = ui.files.selected.filter(|i| *i < l.rows.len());
        ui.dirty = true;
    }
    for m in modals.read() {
        ui.modal = Some(m.0.clone());
        ui.tip_never = false;
        ui.dirty = true;
    }
    for p in pages.read() {
        match p.0.clone() {
            Some(v) => {
                let screen = match v {
                    PageView::Trade(_) => Screen::Trade,
                    PageView::Loot(_) => Screen::Loot,
                    PageView::Recipe(_) => Screen::Recipe,
                    PageView::Document(_) => Screen::Document,
                };
                ui.page = Some(v);
                ui.set_screen(screen);
            }
            None => {
                ui.page = None;
                if matches!(
                    ui.screen,
                    Screen::Trade | Screen::Loot | Screen::Recipe | Screen::Document
                ) {
                    ui.set_screen(Screen::None);
                }
            }
        }
    }
    for t in trades.read() {
        ui.page = None;
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

/// Keep the dialog models in step with the conversation: a new spoken line starts the
/// subtitle (`StartSay`), a new menu restarts the option list on its first row.
pub(crate) fn sync_dialog_models(
    cfg: Res<UiConfig>,
    mut ui: ResMut<UiState>,
    mut seen: Local<(u64, u64)>,
) {
    let Some(d) = ui.dialog.as_ref() else {
        *seen = (0, 0);
        return;
    };
    let hash = |s: &str| {
        s.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        })
    };
    let last = d.lines.last().map(|l| l.text.clone());
    let lines_sig = last.as_deref().map_or(0, hash) ^ (d.lines.len() as u64) << 48;
    let menu_sig = d.menu.iter().fold(d.menu.len() as u64, |h, c| {
        h.wrapping_mul(31) ^ hash(&c.label)
    });
    let menu_len = d.menu.len();
    if lines_sig != seen.0 {
        seen.0 = lines_sig;
        if let Some(t) = last.filter(|t| !t.is_empty()) {
            let width = dialogmenu::approx_text_width(&t, 15.0);
            ui.subtitle.start(&t, cfg.subtitles, false, width);
        }
    }
    if menu_sig != seen.1 {
        seen.1 = menu_sig;
        ui.options = OptionList::new(DIALOG_ROWS);
        ui.options.reset(menu_len);
        ui.dirty = true;
    }
}

/// Visible rows of the dialog option list (169 px band, 24 px rows).
pub(crate) const DIALOG_ROWS: usize = 7;

/// Fade the talk line and age the game messages.
pub(crate) fn tick_models(time: Res<Time>, mut ui: ResMut<UiState>) {
    let dt = time.delta_secs();
    if ui.subtitle.visible() || !ui.subtitle.text.is_empty() {
        ui.subtitle.tick(dt);
    }
}

/// Mouse wheel over the dialog list scrolls its window.
pub(crate) fn dialog_wheel(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut ui: ResMut<UiState>,
) {
    for w in wheel.read() {
        if ui.screen == Screen::Dialog && ui.options.len() > DIALOG_ROWS {
            ui.options.scroll(-w.y.signum() as i32);
            ui.dirty = true;
        }
    }
}
