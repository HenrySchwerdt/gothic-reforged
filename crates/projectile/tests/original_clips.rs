//! The clip lengths in `aim::clips` against the installed game (skipped without an installation).
use gothic_formats::{PakArchive, xmot::Motion};
use gothic_projectile::aim::{Phase, Pitch, clip_name, clips, draw_multiplier};

const PAK: &str = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data\_compiledAnimation.pak";

fn duration(pak: &mut PakArchive, name: &str) -> Option<f32> {
    let at = pak
        .entries
        .iter()
        .position(|e| e.path == format!("{name}.xmot"))?;
    Some(Motion::parse(&pak.read_payload(at).ok()?).ok()?.duration)
}

fn check(pak: &mut PakArchive, left: &str, right: &str, phase: Phase, expect: f32) {
    for pitch in [Pitch::Fwd, Pitch::Up, Pitch::Down] {
        if matches!(phase, Phase::CockBegin) && pitch != Pitch::Fwd {
            continue;
        }
        let name = clip_name("Hero", left, right, phase, pitch);
        let d = duration(pak, &name).unwrap_or_else(|| panic!("missing clip {name}"));
        assert!(
            (d - expect).abs() < 0.005,
            "{name}: {d} s, expected {expect} s"
        );
    }
}

#[test]
fn the_aim_clips_have_the_documented_lengths() {
    let Ok(mut pak) = PakArchive::open(PAK) else {
        eprintln!("no installation: skipped");
        return;
    };
    check(&mut pak, "Bow", "Arrow", Phase::Raise, clips::bow::RAISE);
    check(
        &mut pak,
        "Bow",
        "Arrow",
        Phase::PowerRaise,
        clips::bow::POWER_RAISE,
    );
    check(&mut pak, "Bow", "Arrow", Phase::Aim, clips::bow::AIM);
    check(&mut pak, "Bow", "None", Phase::Recover, clips::bow::RECOVER);
    check(
        &mut pak,
        "Bow",
        "Arrow",
        Phase::ReloadRaise,
        clips::bow::RELOAD_RAISE,
    );
    check(
        &mut pak,
        "CrossBow",
        "Bolt",
        Phase::Raise,
        clips::crossbow::RAISE,
    );
    check(
        &mut pak,
        "CrossBow",
        "Bolt",
        Phase::Aim,
        clips::crossbow::AIM,
    );
    check(
        &mut pak,
        "CrossBow",
        "None",
        Phase::Recover,
        clips::crossbow::RECOVER,
    );
    check(
        &mut pak,
        "CrossBow",
        "Bolt",
        Phase::CockBegin,
        clips::crossbow::COCK,
    );
}

#[test]
fn the_draw_ramp_runs_in_step_with_the_power_raise_clip() {
    // Full strength after 1.95 s; the clip is 2.0 s long.
    assert!((draw_multiplier(clips::bow::POWER_RAISE) - 1.0).abs() < 1e-6);
    assert!(draw_multiplier(clips::bow::POWER_RAISE - 0.2) < 1.0);
}

#[test]
fn the_spell_clips_have_the_documented_lengths() {
    let Ok(mut pak) = PakArchive::open(PAK) else {
        eprintln!("no installation: skipped");
        return;
    };
    for (kind, (raise, hit, recover)) in [
        ("Cast", clips::cast::CAST),
        ("QuickCast", clips::cast::QUICK),
        ("PowerCast", clips::cast::POWER),
    ] {
        for (phase, expect) in [("Raise", raise), ("Hit", hit), ("Recover", recover)] {
            let name = format!("Hero_Stand_Staff_Cast_P0_{kind}_{phase}_N_Fwd_00_%_00_P0_0");
            let d = duration(&mut pak, &name).unwrap_or_else(|| panic!("missing {name}"));
            assert!((d - expect).abs() < 0.005, "{name}: {d}");
        }
    }
}
