//! Debug: generate a tree from an extracted `.spt` and print the mesh as JSON.
use gothic_formats::terrain::{spt::Spt, spt_gen};
fn mesh(m: &spt_gen::MeshOut) -> String {
    format!(
        "{{\"p\":{:?},\"uv\":{:?},\"i\":{:?}}}",
        m.positions, m.uvs, m.indices
    )
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).expect("read");
    let variant = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let spt = Spt::parse(&bytes).expect("parse");
    let g = spt_gen::generate(
        &spt,
        variant,
        spt_gen::GenOptions {
            length_scale: std::env::var("LS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1.0),
            crown_base: std::env::var("CB").ok().and_then(|s| s.parse().ok()),
            ..Default::default()
        },
    );
    println!(
        "{{\"size\":{},\"levels\":{},\"crown\":{},\"composite\":\"{}\",\"bark\":\"{}\",\"bb\":\"{:?}\",\"bark_mesh\":{},\"cards\":{}}}",
        spt.size,
        spt.levels.len(),
        g.crown_radius,
        spt.composite.clone().unwrap_or_default(),
        spt.bark,
        spt.billboard_rect,
        mesh(&g.bark),
        mesh(&g.cards)
    );
}
