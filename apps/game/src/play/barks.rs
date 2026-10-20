//! Stock-voice lines (`SVM_<voice>_<event>`): `Event::Voice` of the ported script states ->
//! `PlayVoice`. The speaking cooldown of the original speak helper runs in the simulation
//! (`gothic_sim::bark`), so every event that arrives here already passed it.
use super::{NpcExtra, PlayState, Roster};
use bevy::prelude::*;
use gothic_audio::{AudioAssets, PlayVoice};
use gothic_runtime::{CoreEvent, CoreRes, Event};

/// What was spoken (smoke report and tests).
#[derive(Resource, Default)]
pub struct BarkState {
    /// `(voice, event)` of every line that was played.
    pub played: Vec<(String, String)>,
    /// Lines without a recording of the speaker's own voice (they stay silent).
    pub missing: u32,
}

pub fn register(app: &mut App) {
    app.add_systems(Update, barks);
}

/// `Confirm_01` -> candidates `Confirm_01`, `Confirm` (numbered families have the index in the
/// sample id, single lines do not).
fn candidates(event: &str) -> Vec<String> {
    let mut v = vec![event.to_string()];
    if let Some((base, idx)) = event.rsplit_once('_')
        && idx.len() == 2
        && idx.chars().all(|c| c.is_ascii_digit())
    {
        v.push(base.to_string());
    }
    v
}

#[allow(clippy::too_many_arguments)]
fn barks(
    mut events: MessageReader<CoreEvent>,
    audio: Option<Res<AudioAssets>>,
    roster: Res<Roster>,
    extras: Query<&NpcExtra>,
    state: Res<PlayState>,
    core: Res<CoreRes>,
    mut play: MessageWriter<PlayVoice>,
    mut bark: ResMut<BarkState>,
) {
    let Some(audio) = audio else { return };
    for e in events.read() {
        let Event::Voice { actor, key } = &e.0 else {
            continue;
        };
        let (voice, entity) = if *actor == core.0.hero {
            ("Player".to_string(), state.hero_entity)
        } else {
            let Some(en) = roster.by_actor.get(actor).copied() else {
                continue;
            };
            let v = extras.get(en).map(|x| x.voice.clone()).unwrap_or_default();
            (v, Some(en))
        };
        if voice.is_empty() {
            continue;
        }
        let found = candidates(key)
            .into_iter()
            .find(|ev| audio.0.speech.has(&format!("SVM_{voice}_{ev}")));
        let Some(event) = found else {
            bark.missing += 1;
            continue;
        };
        let full = format!("SVM_{voice}_{event}");
        play.write(match entity {
            Some(en) => PlayVoice::spatial(full, en),
            None => PlayVoice::new(full),
        });
        bark.played.push((voice, event));
    }
}
