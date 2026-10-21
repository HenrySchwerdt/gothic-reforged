use gothic_render_plus::budget::{OVER_FRAMES, SETTLE_FRAMES};
use gothic_render_plus::{Change, Effect, Governor, Plan, Preset, RenderProfile};

#[test]
fn default_is_faithful_and_plans_nothing() {
    let p = RenderProfile::default();
    assert!(p.is_faithful());
    assert!(Plan::new(&p, &[]).is_empty());
    assert!(Plan::new(&RenderProfile::preset(Preset::Faithful), &[]).is_empty());
    // A settings file without the section, or with only some fields, stays faithful.
    let parsed: RenderProfile = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed, p);
    let partial: RenderProfile = serde_json::from_str(r#"{"ssao":{"radius":1.0}}"#).unwrap();
    assert!(partial.is_faithful());
}

#[test]
fn every_effect_toggles_alone() {
    let high = RenderProfile::preset(Preset::High);
    for e in Effect::ALL {
        let plan = Plan::new(&high.only(e), &[]);
        assert_eq!(plan.active(), vec![e], "{e:?}");
        let off = Plan::new(&high.only(e), &[e]);
        assert!(off.is_empty(), "{e:?} suppressed: {off:?}");
    }
}

#[test]
fn plan_requirements() {
    let high = RenderProfile::preset(Preset::High);
    let all = Plan::new(&high, &[]);
    assert!(all.msaa_off && all.deferred && all.depth_prepass && all.normal_prepass);
    assert!(all.motion_prepass && all.temporal_shadow_filter);
    assert!(all.sharpening.is_some() && all.contact_shadows.is_some());

    // Temporal shadow filtering is noisy without TAA: only together with it.
    let soft = Plan::new(&high.only(Effect::SoftShadows), &[]);
    assert!(!soft.temporal_shadow_filter && !soft.msaa_off && soft.depth_prepass);
    // SSAO needs depth + normals and (in Bevy 0.20) MSAA off, but stays forward.
    let ssao = Plan::new(&high.only(Effect::Ssao), &[]);
    assert!(ssao.msaa_off && ssao.depth_prepass && ssao.normal_prepass && !ssao.deferred);
    // SSR means deferred and no MSAA.
    let ssr = Plan::new(&high.only(Effect::Ssr), &[]);
    assert!(ssr.deferred && ssr.msaa_off);
    // Volumetric fog alone needs no prepass.
    let vol = Plan::new(&high.only(Effect::Volumetric), &[]);
    assert!(!vol.depth_prepass && !vol.msaa_off);
}

#[test]
fn profile_roundtrips_through_json() {
    let p = RenderProfile::preset(Preset::Medium);
    let s = serde_json::to_string_pretty(&p).unwrap();
    assert_eq!(serde_json::from_str::<RenderProfile>(&s).unwrap(), p);
}

/// Simulated GPU: base cost plus the cost of each active effect.
fn frame_ms(base: f32, profile: &RenderProfile, g: &Governor) -> f32 {
    let plan = Plan::new(profile, &g.suppressed());
    base + plan
        .active()
        .iter()
        .map(|e| match e {
            Effect::Ssr => 3.0,
            Effect::Volumetric => 4.0,
            Effect::SoftShadows => 2.0,
            Effect::Ssao => 1.5,
            Effect::Taa => 0.8,
        })
        .sum::<f32>()
}

fn run(
    base: impl Fn(u32) -> f32,
    profile: &RenderProfile,
    g: &mut Governor,
    frames: u32,
) -> Vec<Change> {
    let mut out = Vec::new();
    for f in 0..frames {
        let ms = frame_ms(base(f), profile, g);
        if let Some(c) = g.update(profile, ms) {
            out.push(c);
        }
    }
    out
}

#[test]
fn governor_sheds_in_order_until_under_budget() {
    let mut p = RenderProfile::preset(Preset::High);
    p.budget.target_ms = 16.6;
    let mut g = Governor::default();
    // 8 ms base + 11.3 ms effects = 19.3: dropping SSR (3) gives 16.3, which is within 10 %.
    let changes = run(|_| 8.0, &p, &mut g, 2000);
    assert_eq!(changes, vec![Change::Dropped(Effect::Ssr)]);
    // Heavier scene: also volumetric goes.
    let changes = run(|_| 11.0, &p, &mut g, 2000);
    assert_eq!(changes, vec![Change::Dropped(Effect::Volumetric)]);
    assert_eq!(g.suppressed(), vec![Effect::Ssr, Effect::Volumetric]);
}

#[test]
fn governor_restores_when_there_is_room_and_does_not_oscillate() {
    let p = RenderProfile::preset(Preset::High);
    let mut g = Governor::default();
    run(|_| 11.0, &p, &mut g, 2000);
    assert_eq!(g.suppressed(), vec![Effect::Ssr, Effect::Volumetric]);
    // Light scene: both come back, newest first.
    let changes = run(|_| 2.0, &p, &mut g, 4000);
    assert_eq!(
        changes,
        vec![
            Change::Restored(Effect::Volumetric),
            Change::Restored(Effect::Ssr)
        ]
    );
    // Right at the edge: a scene where SSR would just not fit stays put, with no flapping.
    let mut g = Governor::default();
    let changes = run(|_| 8.0, &p, &mut g, 10_000);
    assert_eq!(changes, vec![Change::Dropped(Effect::Ssr)]);
}

#[test]
fn governor_waits_for_settling_and_ignores_spikes() {
    let p = RenderProfile::preset(Preset::High);
    let mut g = Governor::default();
    // Nothing happens inside the settle window even when far over budget.
    let changes = run(|_| 40.0, &p, &mut g, SETTLE_FRAMES);
    assert!(changes.is_empty());
    // A short hitch (shader compile) in a scene that fits does not drop anything.
    let mut g = Governor::default();
    let changes = run(
        |f| {
            if (300..300 + OVER_FRAMES / 2).contains(&f) {
                60.0
            } else {
                3.0
            }
        },
        &p,
        &mut g,
        1000,
    );
    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn disabling_the_budget_gives_everything_back() {
    let mut p = RenderProfile::preset(Preset::High);
    let mut g = Governor::default();
    run(|_| 11.0, &p, &mut g, 2000);
    assert!(!g.suppressed().is_empty());
    p.budget.enabled = false;
    run(|_| 11.0, &p, &mut g, 10);
    assert!(g.suppressed().is_empty());
    // An effect the user turned off is forgotten, not restored.
    let mut p = RenderProfile::preset(Preset::High);
    let mut g = Governor::default();
    run(|_| 8.0, &p, &mut g, 2000);
    assert_eq!(g.suppressed(), vec![Effect::Ssr]);
    p.set(Effect::Ssr, false);
    run(|_| 1.0, &p, &mut g, 1);
    assert!(g.suppressed().is_empty());
}
