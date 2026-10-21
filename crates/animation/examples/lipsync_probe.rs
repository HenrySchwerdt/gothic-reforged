//! `lipsync_probe HEAD_ACTOR [XLIP_FILE]`: checks the lip-sync pipeline on installed data. Loads
//! the animated head actor and its `_LipSync` motion, builds the speech motion from the entries
//! of an `.xlip` (default: a synthetic one) and reports how many face bones it drives.
use gothic_animation::{
    clip::{Clip, ClipBinding},
    face::{build_lipsync_clip, lipsync_source_name},
    rig::Rig,
};
use gothic_formats::{PakArchive, xact::Actor, xmot::Motion};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let head = args
        .next()
        .expect("head actor name, e.g. G3_Head_Fat_Animated_02");
    let mut pak =
        PakArchive::open(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data\_compiledAnimation.pak")?;
    let find = |pak: &PakArchive, n: &str| {
        pak.entries
            .iter()
            .position(|e| e.path.eq_ignore_ascii_case(n) && !e.deleted())
    };
    let at = find(&pak, &format!("{head}.xact")).expect("head actor");
    let actor = Actor::parse(&pak.read_payload(at)?)?;
    let rig = Rig::from_actor(&head, &actor);
    let at = find(&pak, &format!("{}.xmot", lipsync_source_name(&head))).expect("lipsync motion");
    let source = Clip::from_motion(&head, &Motion::parse(&pak.read_payload(at)?)?);
    let frames: Vec<(f32, u32)> = match args.next() {
        // Same framing as the game's decoder (bare or Genome-wrapped).
        Some(path) => decode_xlip(&std::fs::read(path)?)?,
        None => vec![(0.0, 1), (0.2, 11), (0.45, 9), (0.7, 1)],
    };
    let clip = build_lipsync_clip("speech", &source, &frames).expect("viseme tracks");
    let binding = ClipBinding::new(&rig, &clip);
    println!(
        "head {head}: rig {} nodes / {} slots, source {} nodes, speech clip {} nodes ({:.2} s), bound slots {}",
        rig.names.len(),
        rig.slot_node.len(),
        source.nodes.len(),
        clip.nodes.len(),
        clip.duration,
        binding.bound
    );
    for n in &clip.nodes {
        println!(
            "  {:34} pos {:2} rot {:2} at 0.2 s: {:?}",
            n.name,
            n.pos.len(),
            n.rot.len(),
            n.sample(0.2).r
        );
    }
    Ok(())
}

fn decode_xlip(bytes: &[u8]) -> anyhow::Result<Vec<(f32, u32)>> {
    let body = if bytes.starts_with(b"GENOMFLE") {
        &bytes[14..]
    } else {
        bytes
    };
    let n = u32::from_le_bytes(body[2..6].try_into()?) as usize;
    Ok(body[6..6 + 8 * n]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| {
            (
                f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            )
        })
        .collect())
}
