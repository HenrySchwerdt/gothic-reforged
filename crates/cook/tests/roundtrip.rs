//! Synthetic round-trip tests: no original game data is used or required.
use gothic_cook::actor::{ActorData, MotionData, cook_motion};
use gothic_cook::cache::{Cache, Manifest, write_atomic};
use gothic_cook::ids::{AssetId, Profile};
use gothic_cook::material::{MaterialRec, MaterialTable, write_table};
use gothic_cook::mesh::{MeshData, cook_mesh};
use gothic_cook::mmap::Blob;
use gothic_cook::texture::{TexFormat, TextureData, cook_texture};
use gothic_cook::world::{
    CellBuilder, CellData, CollisionBuilder, CollisionPack, Instance, TerrainRef, bevy_transform,
};
use gothic_formats::xcmsh::{Submesh, Xcmsh};
use gothic_formats::xmot::{Motion, MotionNode, Track};
use std::sync::Arc;

fn grid_mesh(n: usize) -> Xcmsh {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    for y in 0..=n {
        for x in 0..=n {
            positions.push([x as f32 * 10.0, ((x * y) % 7) as f32, y as f32 * 10.0]);
            uvs.push([x as f32 / n as f32, y as f32 / n as f32]);
        }
    }
    let mut indices = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let i = (y * (n + 1) + x) as u32;
            let w = (n + 1) as u32;
            indices.extend([i, i + 1, i + w, i + 1, i + w + 1, i + w]);
        }
    }
    let count = positions.len();
    Xcmsh {
        submeshes: vec![Submesh {
            normals: vec![[1.0, 0.0, 0.0]; count],
            positions,
            uvs,
            indices,
            material: "G3_Test_Material.xshmat".into(),
            bounds_min: [0.0; 3],
            bounds_max: [0.0; 3],
            colors4: vec![],
            colors5: vec![],
            tangents: vec![],
            uv2: vec![],
        }],
    }
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gothic-cook-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn mesh_roundtrip_mirrors_x_and_builds_lods() {
    let src = grid_mesh(40);
    let profile = Profile::by_name("desktop-high").unwrap();
    let c = cook_mesh(&src, &profile, false, 7).unwrap();
    assert_eq!(c.material_names, vec!["G3_Test_Material.xshmat"]);
    let m = MeshData::parse(Arc::new(Blob::from_bytes(&c.bytes))).unwrap();
    let sub = m.submeshes()[0];
    assert_eq!(sub.material, AssetId::of("g3_test_material.xshmat").0);
    assert!(sub.lod_count >= 2, "expected LODs, got {}", sub.lod_count);
    let l0 = m.indices(&sub, 0);
    let l1 = m.indices(&sub, 1);
    assert_eq!(l0.len(), 40 * 40 * 6);
    assert!(l1.len() < l0.len());
    let verts = m.submesh_vertices(&sub);
    assert!((0..l1.len()).all(|i| (l1.get(i) as usize) < verts.len()));
    // X mirrored; normals mirrored; max x of source is 400 -> min x is -400.
    assert!((m.header().bounds_min[0] + 400.0).abs() < 1e-3);
    assert!(verts.iter().all(|v| v.normal == [-1.0, 0.0, 0.0]));
    // Terrain meshes get no LODs.
    let t = cook_mesh(&src, &profile, true, 7).unwrap();
    let tm = MeshData::parse(Arc::new(Blob::from_bytes(&t.bytes))).unwrap();
    assert!(tm.is_terrain() && tm.submeshes()[0].lod_count == 1);
}

#[test]
fn rejects_corrupt_and_truncated_files() {
    let c = cook_mesh(&grid_mesh(4), &Profile::by_name("quest").unwrap(), false, 1).unwrap();
    for n in [0, 10, 31, 40, c.bytes.len() / 2] {
        assert!(MeshData::parse(Arc::new(Blob::from_bytes(&c.bytes[..n]))).is_err());
    }
    let mut bad = c.bytes.clone();
    bad[0] = b'X';
    assert!(MeshData::parse(Arc::new(Blob::from_bytes(&bad))).is_err());
    assert!(
        gothic_cook::texture::TextureData::parse(Arc::new(Blob::from_bytes(&c.bytes))).is_err()
    );
}

fn ximg(w: u32, h: u32, mips: u32) -> Vec<u8> {
    let mut b = vec![0; 87];
    b[..8].copy_from_slice(b"GENOMFLE");
    b[8] = 1;
    b[38..43].copy_from_slice(b"G3IMG");
    b[47..51].copy_from_slice(&w.to_le_bytes());
    b[51..55].copy_from_slice(&h.to_le_bytes());
    b[63..67].copy_from_slice(&mips.to_le_bytes());
    b[71..75].copy_from_slice(b"DXT1");
    for level in (0..mips).rev() {
        let size = ((w >> level).max(1).div_ceil(4) * (h >> level).max(1).div_ceil(4)) as usize * 8;
        b.extend(vec![(level as u8) * 16 + 1; size]);
    }
    let table = b.len() as u32;
    b[10..14].copy_from_slice(&table.to_le_bytes());
    b.extend([0xef, 0xbe, 0xad, 0xde, 1, 0, 0, 0, 0]);
    b
}

#[test]
fn texture_profiles_keep_blocks_and_cap_resolution() {
    let src = ximg(2048, 2048, 12);
    let high = cook_texture(&src, &Profile::by_name("desktop-high").unwrap(), true, 1).unwrap();
    let t = TextureData::parse(Arc::new(Blob::from_bytes(&high.bytes))).unwrap();
    assert_eq!((t.header().width, t.format()), (2048, TexFormat::Bc1));
    assert_eq!(t.mips().len(), 12);
    assert_eq!(t.mip_data(0).unwrap().len(), 512 * 512 * 8);
    assert!(t.mip_data(0).unwrap().iter().all(|&b| b == 1));

    let low = cook_texture(&src, &Profile::by_name("desktop-low").unwrap(), true, 1).unwrap();
    let t = TextureData::parse(Arc::new(Blob::from_bytes(&low.bytes))).unwrap();
    assert_eq!((t.header().width, t.header().source_width), (1024, 2048));
    assert_eq!(t.mips().len(), 11);
}

#[cfg(feature = "astc")]
#[test]
fn quest_profile_encodes_astc_6x6() {
    let src = ximg(2048, 1024, 5);
    let q = cook_texture(&src, &Profile::by_name("quest").unwrap(), true, 1).unwrap();
    let t = TextureData::parse(Arc::new(Blob::from_bytes(&q.bytes))).unwrap();
    assert_eq!(t.format(), TexFormat::Astc6x6);
    assert_eq!((t.header().width, t.header().height), (1024, 512));
    // 1024x512 -> ceil(1024/6) * ceil(512/6) blocks of 16 bytes.
    assert_eq!(t.mip_data(0).unwrap().len(), 171 * 86 * 16);
    assert_eq!(t.mips().len(), 4);
}

#[test]
fn cell_and_collision_roundtrip() {
    let m = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 2.0, 0.0, 0.0],
        [0.0, 0.0, 3.0, 0.0],
        [100.0, 200.0, 300.0, 1.0],
    ];
    let t = bevy_transform(&m);
    assert_eq!(t[3], [-100.0, 200.0, 300.0]);
    assert_eq!(t[0], [1.0, 0.0, 0.0]);
    let mut b = CellBuilder::new(9, -2);
    b.instances.push(Instance {
        mesh: 5,
        collision: 6,
        transform: t,
    });
    b.terrain.push(TerrainRef {
        mesh: 8,
        bounds_min: [0.0; 3],
        bounds_max: [1.0; 3],
    });
    b.collision
        .extend([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    let bytes = b.write(3);
    let c = CellData::parse(Arc::new(Blob::from_bytes(&bytes))).unwrap();
    assert_eq!((c.header().cx, c.header().cz), (9, -2));
    assert_eq!(c.instances()[0].mesh, 5);
    assert_eq!(c.terrain()[0].mesh, 8);
    assert_eq!(c.collision_triangles().count(), 1);

    let mut cb = CollisionBuilder::default();
    cb.add(
        20,
        &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        &[0, 1, 2],
    );
    cb.add(10, &[[5.0; 3]], &[0, 0, 0]);
    let p = CollisionPack::parse(Arc::new(Blob::from_bytes(&cb.finish(1)))).unwrap();
    assert_eq!(p.get(20).unwrap().indices, &[0, 1, 2]);
    assert_eq!(p.get(10).unwrap().vertices, &[[5.0; 3]]);
    assert!(p.get(11).is_none());
}

#[test]
fn material_table_binary_search() {
    let recs: Vec<MaterialRec> = [9u64, 3, 7]
        .iter()
        .map(|&id| MaterialRec {
            id,
            blend_mode: id as u32,
            ..Default::default()
        })
        .collect();
    let t = MaterialTable::parse(Arc::new(Blob::from_bytes(&write_table(recs, 1)))).unwrap();
    assert_eq!(t.get(7).unwrap().blend_mode, 7);
    assert!(t.get(8).is_none());
}

#[test]
fn motion_quantization_stays_close() {
    let node = MotionNode {
        name: "Bip01".into(),
        position: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
        positions: Track::default(),
        rotations: Track {
            keys: vec![(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [0.5, 0.5, 0.5, 0.5])],
        },
        scales: Track::default(),
        dropped_keys: 0,
    };
    let m = Motion {
        nodes: vec![node],
        duration: 1.0,
        key_count: 2,
        min_key_interval: 1.0,
    };
    for name in ["desktop-high", "quest"] {
        let bytes = cook_motion(&m, &Profile::by_name(name).unwrap(), 1).unwrap();
        let d = MotionData::parse(Arc::new(Blob::from_bytes(&bytes))).unwrap();
        let keys = d.rotation_keys(&d.nodes()[0]);
        assert_eq!(keys.len(), 2);
        for (a, b) in keys[1].1.iter().zip([0.5; 4]) {
            assert!((a - b).abs() < 1e-3, "{name}");
        }
    }
    let _ = ActorData::parse;
}

#[test]
fn cache_reader_over_files_and_atomic_writes() {
    let dir = tmp("cache");
    let c = cook_mesh(
        &grid_mesh(4),
        &Profile::by_name("desktop-high").unwrap(),
        false,
        1,
    )
    .unwrap();
    write_atomic(
        &dir.join(gothic_cook::cache::mesh_path(AssetId::of("A.xcmsh"))),
        &c.bytes,
    )
    .unwrap();
    let man = Manifest {
        cooker_version: gothic_cook::ids::COOKER_VERSION,
        ..Default::default()
    };
    man.save(&dir).unwrap();
    let cache = Cache::open(&dir).unwrap();
    // Case-insensitive, path-insensitive id.
    assert!(cache.load_mesh_named("some/dir/a.XCMSH").is_ok());
    assert!(cache.load_mesh_named("missing.xcmsh").is_err());
    assert!(cache.load_cell(0, 0).unwrap().is_none());
    assert!(Cache::open(dir.join("nope")).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
