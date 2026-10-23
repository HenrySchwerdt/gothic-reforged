//! Host tool: lists the submeshes of a cooked actor (node, skinned, bounds in model space, cm) from the local quest cache.
use gothic_cook::{AssetId, cache::Cache};
fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "g3_orc_body_warrior.xact".into());
    let dir = gothic_quest::ardea::data_dir().expect("pack");
    let cache = Cache::open(&dir).expect("cache");
    let data = cache.load_actor(AssetId::of(&name)).expect("actor");
    let names = data.node_names();
    let verts = data.vertices();
    for (i, s) in data.submeshes().iter().enumerate() {
        let vs = &verts[s.first_vertex as usize..(s.first_vertex + s.vertex_count) as usize];
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in vs {
            for k in 0..3 {
                lo[k] = lo[k].min(v.pos[k]);
                hi[k] = hi[k].max(v.pos[k]);
            }
        }
        println!(
            "sub {i:2} node {:28} skinned {} verts {:5} mat {:2} y {:7.1}..{:7.1} x {:6.1}..{:6.1} z {:6.1}..{:6.1}",
            names
                .get(s.node as usize)
                .map(String::as_str)
                .unwrap_or("?"),
            s.skinned,
            s.vertex_count,
            s.material,
            lo[1],
            hi[1],
            lo[0],
            hi[0],
            lo[2],
            hi[2]
        );
    }
}
