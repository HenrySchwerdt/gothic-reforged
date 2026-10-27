//! The hero's use of the world: what the confirm key did, interact objects, loose items, loot
//! and work-station windows, and the player events of the ported `PS_*` states turned into
//! poses, sounds, toasts and windows.
use super::objects::WorldObjects;
use super::pose::{self, Posed};
use super::{HeroActor, Mode, PlayState, Roster};
use crate::walk::Player;
use bevy::prelude::*;
use gothic_animation::{ActorLibrary, Animator, Stance};
use gothic_audio::PlaySfx;
use gothic_gameplay::treasure::{HeroSkills, Roll, generate_plunder_inventory};
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event};
use gothic_sim::script::enums::UseType;
use gothic_sim::script::host::EntityId;
use gothic_sim::script::player::api::{PlayerEvent, Refusal, SkillCategory, UiWindow};
use gothic_sim::script::player::craft::Synth;
use gothic_sim::script::player::interact::{Mode as StationMode, station_for_use};
use gothic_sim::script::player::normal::Confirm;
use gothic_sim::sim::EventKind;
use gothic_sim::types::ActorId;
use gothic_ui::{Notify, ToastKind};

/// What a line of an open window does when picked.
#[derive(Clone, Debug)]
pub enum LineAction {
    Take(usize),
    Craft(i32),
}

#[derive(Clone, Debug)]
pub struct WindowLine {
    pub text: String,
    pub action: LineAction,
    /// Item template shown as the cell icon.
    pub item: String,
    pub count: i32,
}

#[derive(Clone, Debug)]
pub enum WindowKind {
    /// Contents of a chest or other container (script entity id).
    Loot { object: u32 },
    /// Plunder of a body.
    Body { actor: ActorId },
    /// Recipes of a work station.
    Station { window: UiWindow },
    /// The trade page with a merchant (`gCHUDPageTrade2`).
    Trade { npc: ActorId },
}

/// A window of the play mode (loot, plunder, recipes, trade). It is drawn by the HUD pages of the
/// UI crate (`windows.rs`); `lines` is the model of the loot-style pages.
#[derive(Clone, Debug)]
pub struct OpenWindow {
    pub kind: WindowKind,
    pub title: String,
    pub lines: Vec<WindowLine>,
    pub footer: String,
    pub dirty: bool,
}

#[derive(Resource, Default)]
pub struct Interaction {
    /// Interact object in use (index into [`WorldObjects`]).
    pub active: Option<usize>,
    /// Loose item the hero is picking up.
    pub pending_item: Option<usize>,
    pub window: Option<OpenWindow>,
    pub window_key: Option<usize>,
    pub window_enter: bool,
    pub window_close: bool,
    /// A merchant whose trade page opens next (`TradeOpened` of a dialog).
    pub pending_trade: Option<ActorId>,
    /// The deal key (Enter) was pressed on the trade page.
    pub trade_deal: bool,
    /// The UI shows the page of the open window.
    pub page_shown: bool,
    /// The hero holds a seat / bed / station pose until he leaves.
    pub holding_pose: bool,
    pub release_pose: bool,
    refresh_at: Option<f32>,
    /// Event log for the smoke report: (kind, detail).
    pub log: Vec<String>,
}

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        (start_trade, handle_events, window_input, refresh_window).chain(),
    );
}

fn actor_of(e: EntityId) -> ActorId {
    ActorId(e.0.saturating_sub(1))
}

fn item_name(core: &CoreRes, template: &str) -> String {
    core.0
        .db
        .text(&format!("FO_{template}"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| template.replace('_', " "))
}

/// The pose a use type puts the hero in: `(action, loop seconds, held until he leaves)`.
fn pose_for(use_type: UseType) -> Option<(String, Option<f32>, bool)> {
    match use_type {
        UseType::Chest => Some(("OpenChest".into(), None, false)),
        UseType::Door => Some(("OpenDoor".into(), Some(1.0), false)),
        UseType::Bed => Some(("SleepBed".into(), None, true)),
        other => {
            let st = station_for_use(other)?;
            Some(match st.mode {
                StationMode::Window(_) => (other.name().to_string(), None, false),
                StationMode::Timed(a, secs) => (a.name().to_string(), Some(secs as f32), false),
                StationMode::Pose(a) => (a.name().to_string(), None, true),
            })
        }
    }
}

fn sfx(w: &mut MessageWriter<PlaySfx>, name: &str, at: Option<Vec3>) {
    w.write(PlaySfx::new(name, at));
}

#[allow(clippy::too_many_arguments)]
fn handle_events(
    mut events: MessageReader<CoreEvent>,
    mut core: ResMut<CoreRes>,
    objects: Res<WorldObjects>,
    mut inter: ResMut<Interaction>,
    mut state: ResMut<PlayState>,
    player: Res<Player>,
    mut lib: ResMut<ActorLibrary>,
    hero: Query<(Entity, &Animator), With<HeroActor>>,
    mut posed: Query<&mut Posed, With<HeroActor>>,
    roster: Res<Roster>,
    mut commands: Commands,
    mut cmd: MessageWriter<CoreCommand>,
    mut notify: MessageWriter<Notify>,
    mut sound: MessageWriter<PlaySfx>,
    time: Res<Time>,
) {
    let say = |notify: &mut MessageWriter<Notify>, text: String| {
        notify.write(Notify {
            text,
            kind: ToastKind::Info,
        });
    };
    let hero_pos = player.ctrl.position * 0.01;
    if inter.log.len() > 400 {
        inter.log.drain(..200);
    }
    if std::mem::take(&mut inter.release_pose)
        && let Ok(mut p) = posed.single_mut()
    {
        p.release();
    }
    for e in events.read() {
        match &e.0 {
            Event::Confirmed(c) => {
                inter.log.push(format!("confirm {c:?}"));
                match c {
                    Confirm::Talk(who) => {
                        let npc = actor_of(*who);
                        state.mode = Mode::Dialog;
                        state.dialog_npc = Some(npc);
                        cmd.write(CoreCommand(Command::Talk { npc }));
                    }
                    Confirm::Interact(obj) => {
                        inter.active = objects.index_of_entity(obj.0);
                        cmd.write(CoreCommand(Command::UseInteractObject { object: obj.0 }));
                    }
                    Confirm::PickUp(item) => {
                        inter.pending_item = objects.index_of_entity(item.0);
                        cmd.write(CoreCommand(Command::PickUp { item: item.0 }));
                    }
                    Confirm::Loot(body) => {
                        let a = actor_of(*body);
                        ensure_plunder(&mut core, &objects, a);
                        let actor = core.0.sim.actor(a);
                        let lines = body_lines(&core, a);
                        inter.window = Some(OpenWindow {
                            kind: WindowKind::Body { actor: a },
                            title: actor.name.replace('_', " "),
                            lines,
                            footer: "Enter: alles nehmen    Esc: schliessen".into(),
                            dirty: true,
                        });
                        cmd.write(CoreCommand(Command::GuiOpen(true)));
                    }
                    Confirm::Rob(_) => say(
                        &mut notify,
                        "Du kannst ihn bestehlen (noch nicht umgesetzt)".into(),
                    ),
                    Confirm::NothingToGet(_) | Confirm::TalkRefused(_) | Confirm::Nothing => {}
                }
            }
            Event::Player(p) => {
                inter.log.push(format!("player {p:?}"));
                match p {
                    // The quick slot and potion keys of `PS_Melee_Loop` (hero.rs performs them).
                    PlayerEvent::QuickSlot(_) | PlayerEvent::PotionKey(_) => {}
                    PlayerEvent::InteractStarted { object } => {
                        let idx = objects.index_of_entity(object.0);
                        inter.active = idx;
                        if let Some(i) = idx {
                            let ut = objects.list[i].def.script_use();
                            if let Some((action, secs, hold)) = pose_for(ut)
                                && let Ok((en, anim)) = hero.single()
                            {
                                let family = anim.rig.family.clone();
                                if let Some(p) =
                                    pose::build(&lib, &family, Stance::Unarmed, &action, secs, None)
                                {
                                    commands.entity(en).insert(p);
                                    inter.holding_pose = hold;
                                }
                            }
                            if ut == UseType::Anvil {
                                sfx(
                                    &mut sound,
                                    "sfx_ani_anvil_A00",
                                    Some(objects.list[i].pos_cm * 0.01),
                                );
                            }
                        }
                    }
                    PlayerEvent::InteractEnded { .. } => {
                        inter.active = None;
                        inter.holding_pose = false;
                        if let Ok(mut p) = posed.single_mut() {
                            p.release();
                        }
                        state.drawn = false;
                    }
                    PlayerEvent::Window(w) => {
                        let obj = inter.active.and_then(|i| objects.list[i].entity);
                        match (w, obj) {
                            (UiWindow::Loot, Some(object)) => {
                                sfx(&mut sound, "sfx_ani_chest_open_01_A00", Some(hero_pos));
                                inter.window = Some(loot_window(&core, &objects, object));
                            }
                            (UiWindow::Smithing | UiWindow::Alchemy | UiWindow::Cooking, _) => {
                                inter.window = Some(station_window(&core, &objects, *w));
                            }
                            (other, _) => {
                                say(
                                    &mut notify,
                                    format!("Fenster {other:?}: Seite noch nicht umgesetzt"),
                                );
                                cmd.write(CoreCommand(Command::GuiOpen(false)));
                            }
                        }
                    }
                    PlayerEvent::CloseWindow => inter.window = None,
                    PlayerEvent::DoorChanged { open, .. } => {
                        say(
                            &mut notify,
                            if *open {
                                "Tuer geoeffnet"
                            } else {
                                "Tuer geschlossen"
                            }
                            .into(),
                        );
                    }
                    PlayerEvent::LockPicked { success, .. } => {
                        if *success {
                            sfx(&mut sound, "sfx_ani_chest_unlock_A00", Some(hero_pos));
                            say(&mut notify, "Schloss geoeffnet".into());
                        } else {
                            sfx(&mut sound, "sfx_inv_deny_01", None);
                            say(&mut notify, "Der Dietrich ist abgebrochen".into());
                        }
                    }
                    PlayerEvent::PickedUp { template, .. } => {
                        sfx(&mut sound, "sfx_inv_click_01", None);
                        notify.write(Notify {
                            text: format!("+1 {}", item_name(&core, template)),
                            kind: ToastKind::Item,
                        });
                        if let Some(i) = inter.pending_item.take()
                            && let Some(o) = objects.list.get(i)
                            && let Some(v) = o.visual
                        {
                            commands.entity(v).despawn();
                        }
                    }
                    PlayerEvent::Crafted { template, amount } => {
                        sfx(&mut sound, "sfx_inv_click_02", None);
                        notify.write(Notify {
                            text: format!("Hergestellt: {amount} x {}", item_name(&core, template)),
                            kind: ToastKind::Item,
                        });
                    }
                    PlayerEvent::Refused(r) => {
                        eprintln!("play: interaction refused: {r:?}");
                        // A refused start never began, so no object is in use.
                        if !inter.holding_pose && inter.window.is_none() {
                            inter.active = None;
                        }
                        sfx(&mut sound, "sfx_inv_deny_01", None);
                        if let Some(t) = refusal_text(&core, r) {
                            say(&mut notify, t);
                        }
                    }
                    PlayerEvent::Slept { hours } => {
                        say(
                            &mut notify,
                            format!("Du hast {hours:.0} Stunden geschlafen"),
                        );
                    }
                    PlayerEvent::BookRead { book } => {
                        say(&mut notify, format!("Buch gelesen: {book}"))
                    }
                    PlayerEvent::TimeScale(_) => {}
                }
            }
            Event::CraftResult(r) => {
                inter.refresh_at = Some(time.elapsed_secs() + 0.1);
                match r {
                    Synth::MissingIngredients => say(
                        &mut notify,
                        core.0
                            .db
                            .text("HUD_NoIngredients")
                            .unwrap_or_else(|| "Zutaten fehlen".into()),
                    ),
                    Synth::MissingSkill(p) => say(
                        &mut notify,
                        core.0
                            .db
                            .text("HUD_MissingSkill")
                            .unwrap_or_default()
                            .replace("$(name)", p),
                    ),
                    Synth::Crafted { .. } | Synth::Unknown => {}
                }
            }
            Event::Sim(EventKind::TradeOpened { npc }) => {
                if inter.window.is_none() {
                    inter.pending_trade = Some(*npc);
                }
            }
            Event::ItemUsed(o) => inter.log.push(format!("used {o:?}")),
            Event::CastStarted(st) => inter.log.push(format!("cast started {st:?}")),
            Event::CastRefused(err) => {
                inter.log.push(format!("cast refused {err:?}"));
                say(&mut notify, "Kein Zauber bereit".into());
            }
            Event::Projectile { spell, .. } => inter.log.push(format!("projectile spell={spell}")),
            Event::Notice(t) => inter.log.push(format!("notice {t}")),
            _ => {}
        }
    }
    let _ = (&roster, &mut lib);
}

fn refusal_text(core: &CoreRes, r: &Refusal) -> Option<String> {
    let db = &core.0.db;
    match r {
        Refusal::MissingItem(t) => Some(format!(
            "{}{}",
            db.text("HUD_MissingItem_Interact").unwrap_or_default(),
            item_name(core, t)
        )),
        Refusal::MissingPerk(p) => Some(
            db.text("HUD_MissingSkill")
                .unwrap_or_default()
                .replace("$(name)", p),
        ),
        Refusal::MissingIngredients => db.text("HUD_NoIngredients"),
        Refusal::NoRecipes(c) => Some(format!("Keine Rezepte ({c:?})")),
        // The other refusals are spoken by the hero (`NeedKey`, `NothingToGet`, ...).
        _ => None,
    }
}

fn loot_window(core: &CoreRes, _objects: &WorldObjects, object: u32) -> OpenWindow {
    let lines = core
        .0
        .sim
        .script
        .object(EntityId(object))
        .map(|o| {
            o.loot
                .iter()
                .enumerate()
                .map(|(i, (item, n))| WindowLine {
                    text: format!("{n} x {}", item_name(core, item)),
                    action: LineAction::Take(i),
                    item: item.clone(),
                    count: *n as i32,
                })
                .collect()
        })
        .unwrap_or_default();
    OpenWindow {
        kind: WindowKind::Loot { object },
        title: core
            .0
            .sim
            .script
            .object(EntityId(object))
            .map_or_else(String::new, |o| o.name.clone()),
        lines,
        footer: "1-9: nehmen    Enter: alles nehmen    Esc: schliessen".into(),
        dirty: true,
    }
}

/// Recipes of the station's category whose blueprint the hero carries.
fn station_window(core: &CoreRes, objects: &WorldObjects, window: UiWindow) -> OpenWindow {
    let category = match window {
        UiWindow::Smithing => SkillCategory::Smith,
        UiWindow::Alchemy => SkillCategory::Alchemy,
        _ => SkillCategory::Cooking,
    };
    let hero = core.0.sim.actor(core.0.hero);
    let mut lines = Vec::new();
    for (item, id) in &objects.recipe_ids {
        if !hero
            .inventory
            .iter()
            .any(|(k, n)| *n > 0 && k.eq_ignore_ascii_case(item))
        {
            continue;
        }
        let Some(r) = core
            .0
            .sim
            .script
            .recipe_of(*id)
            .filter(|r| r.category == category)
        else {
            continue;
        };
        let ing: Vec<String> = r
            .ingredients
            .iter()
            .map(|(t, n)| format!("{n} {}", item_name(core, t)))
            .collect();
        lines.push(WindowLine {
            text: format!("{} ({})", item_name(core, &r.result), ing.join(", ")),
            action: LineAction::Craft(*id),
            item: r.result.clone(),
            count: 1,
        });
    }
    lines.sort_by(|a, b| a.text.cmp(&b.text));
    OpenWindow {
        kind: WindowKind::Station { window },
        title: match window {
            UiWindow::Smithing => "Schmieden",
            UiWindow::Alchemy => "Alchemie",
            _ => "Kochen",
        }
        .into(),
        lines,
        footer: "1-9: herstellen    Esc: schliessen".into(),
        dirty: true,
    }
}

/// Stacks of a body's inventory as loot lines (equipped and in-use stacks stay with it).
fn body_lines(core: &CoreRes, body: ActorId) -> Vec<WindowLine> {
    core.0
        .sim
        .actor(body)
        .inventory
        .stacks()
        .iter()
        .enumerate()
        .filter(|(_, s)| s.amount > 0 && s.is_disposable())
        .map(|(i, s)| WindowLine {
            text: format!("{} x {}", s.amount, item_name(core, &s.template)),
            action: LineAction::Take(i),
            item: s.template.clone(),
            count: s.amount,
        })
        .collect()
}

/// `gCInventory_PS::GeneratePlunderInvFromTreasureSets` (Game:0x100b71f0): a body's plunder is
/// rolled once, when somebody first looks into it, with the hero's skills of that moment.
pub fn ensure_plunder(core: &mut CoreRes, objects: &WorldObjects, body: ActorId) {
    let inv = &core.0.sim.actor(body).inventory;
    if inv.generated_plunder {
        return;
    }
    let sets: Vec<String> = inv.treasure_sets.to_vec();
    let mut book = objects.treasure.book(&core.0.sim.items, &sets);
    let mut dice = Roll(super::data::seed_of(&core.0.sim.actor(body).name));
    let sim = &mut core.0.sim;
    let hero_perks = sim.hero.map(|h| sim.actor(h).perks.clone());
    struct Perks(Option<std::collections::BTreeSet<String>>);
    impl HeroSkills for Perks {
        fn is_skill_active(&self, skill: &str) -> Option<bool> {
            self.0
                .as_ref()
                .map(|p| p.contains(&skill.to_ascii_lowercase()))
        }
    }
    generate_plunder_inventory(
        &mut sim.actor_mut(body).inventory,
        &mut book,
        &mut dice,
        &Perks(hero_perks),
    );
}

/// Open the trade page with a merchant (`gCGUIManager::BeginTrade`): stock the merchant from
/// its trade treasure sets once, then show the baskets.
pub fn open_trade(
    core: &mut CoreRes,
    objects: &WorldObjects,
    inter: &mut Interaction,
    cmd: &mut MessageWriter<CoreCommand>,
    npc: ActorId,
) {
    let sets = core.0.sim.actor(npc).inventory.treasure_sets.clone();
    let book = objects.treasure.book(&core.0.sim.items, &sets);
    let mut dice = Roll(super::data::seed_of(&core.0.sim.actor(npc).name));
    core.0.sim.trade_open(npc, &book, &mut dice);
    let title = core.0.sim.actor(npc).name.replace('_', " ");
    inter.window = Some(OpenWindow {
        kind: WindowKind::Trade { npc },
        title,
        lines: Vec::new(),
        footer: String::new(),
        dirty: true,
    });
    cmd.write(CoreCommand(Command::GuiOpen(true)));
}

/// Keys of an open window -> core commands.
#[allow(clippy::too_many_arguments)]
fn window_input(
    mut inter: ResMut<Interaction>,
    mut core: ResMut<CoreRes>,
    time: Res<Time>,
    mut cmd: MessageWriter<CoreCommand>,
    mut notify: MessageWriter<Notify>,
) {
    let Some(win) = inter.window.clone() else {
        inter.window_key = None;
        inter.window_enter = false;
        inter.window_close = false;
        return;
    };
    let now = time.elapsed_secs();
    if std::mem::take(&mut inter.window_close) {
        if let WindowKind::Trade { npc } = win.kind {
            core.0.sim.trade_cancel(npc);
            core.0.sync_inventory();
        }
        cmd.write(CoreCommand(Command::GuiOpen(false)));
        inter.window = None;
        inter.window_key = None;
        inter.window_enter = false;
        return;
    }
    // Digit keys 1..9, 0 pick the n-th entry (gCHUDListCtrlInvLootStacks key handler); a click
    // carries the row itself.
    let picked = inter.window_key.take().and_then(|n| match n {
        1..=10 => char::from_digit((n % 10) as u32, 10)
            .and_then(|c| gothic_gameplay::trade::loot_key_index(c, win.lines.len())),
        _ => Some(n - 1),
    });
    if let Some(line) = picked.and_then(|i| win.lines.get(i)) {
        match (&win.kind, &line.action) {
            (WindowKind::Loot { object }, LineAction::Take(i)) => {
                cmd.write(CoreCommand(Command::TakeLoot {
                    object: *object,
                    index: Some(*i),
                }));
            }
            (WindowKind::Body { actor }, LineAction::Take(i)) => {
                take_from_body(&mut core, *actor, Some(*i), &mut notify);
            }
            (WindowKind::Station { .. }, LineAction::Craft(id)) => {
                cmd.write(CoreCommand(Command::Craft { recipe: *id }));
            }
            _ => {}
        }
        inter.refresh_at = Some(now + 0.15);
    }
    if std::mem::take(&mut inter.window_enter) {
        match &win.kind {
            WindowKind::Loot { object } => {
                cmd.write(CoreCommand(Command::TakeLoot {
                    object: *object,
                    index: None,
                }));
            }
            WindowKind::Body { actor } => {
                take_from_body(&mut core, *actor, None, &mut notify);
            }
            WindowKind::Station { .. } => {
                if let Some(WindowLine {
                    action: LineAction::Craft(id),
                    ..
                }) = win.lines.first()
                {
                    cmd.write(CoreCommand(Command::Craft { recipe: *id }));
                }
            }
            WindowKind::Trade { .. } => inter.trade_deal = true,
        }
        inter.refresh_at = Some(now + 0.15);
    }
}

/// Move one stack (or all) of a body into the hero's inventory and tell the player.
fn take_from_body(
    core: &mut CoreRes,
    body: ActorId,
    idx: Option<usize>,
    notify: &mut MessageWriter<Notify>,
) {
    let taken = core.0.sim.loot_body(body, idx);
    core.0.sync_inventory();
    for (template, n) in taken {
        notify.write(Notify {
            text: format!("+{n} {}", item_name(core, &template)),
            kind: ToastKind::Item,
        });
    }
}

/// Rebuild the lines of an open window from the core state after a take / craft.
fn refresh_window(
    mut inter: ResMut<Interaction>,
    core: Res<CoreRes>,
    objects: Res<WorldObjects>,
    time: Res<Time>,
) {
    let Some(at) = inter.refresh_at else { return };
    if time.elapsed_secs() < at {
        return;
    }
    inter.refresh_at = None;
    let Some(win) = inter.window.as_ref() else {
        return;
    };
    let fresh = match &win.kind {
        WindowKind::Loot { object } => Some(loot_window(&core, &objects, *object)),
        WindowKind::Station { window } => Some(station_window(&core, &objects, *window)),
        WindowKind::Body { actor } => Some(OpenWindow {
            lines: body_lines(&core, *actor),
            dirty: true,
            ..win.clone()
        }),
        WindowKind::Trade { .. } => Some(OpenWindow {
            dirty: true,
            ..win.clone()
        }),
    };
    if let Some(f) = fresh {
        inter.window = Some(f);
    }
}

/// A dialog asked for the trade page (`DoTrade`): end the conversation and open the page.
fn start_trade(
    mut inter: ResMut<Interaction>,
    mut core: ResMut<CoreRes>,
    objects: Res<WorldObjects>,
    state: Res<PlayState>,
    mut cmd: MessageWriter<CoreCommand>,
) {
    let Some(npc) = inter.pending_trade.take() else {
        return;
    };
    if state.mode == Mode::Dialog {
        cmd.write(CoreCommand(Command::EndDialog));
    }
    open_trade(&mut core, &objects, &mut inter, &mut cmd, npc);
}
