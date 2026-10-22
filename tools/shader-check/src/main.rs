use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use bevy_asset::AssetId;
use bevy_shader::{
    Shader, ShaderCache, ShaderCacheError, ShaderCacheSource, ShaderDefVal, ValidateShader,
};
use uuid::Uuid;

type AnyResult<T> = Result<T, Box<dyn Error>>;

fn main() -> AnyResult<()> {
    let (repo, registry_src) = arguments()?;
    let mut cache = ShaderCache::new((), validate_wgsl);
    let mut next_id = 1_u128;

    let bevy_module_count = register_bevy_modules(&mut cache, &registry_src, &mut next_id)?;
    let shader_root = repo.join("apps/game/src/walk/shaders");
    let vertex =
        register_game_shader(&mut cache, &shader_root, "gothic_vertex.wesl", &mut next_id)?;
    let prepass = register_game_shader(
        &mut cache,
        &shader_root,
        "gothic_prepass.wesl",
        &mut next_id,
    )?;
    register_game_shader(&mut cache, &shader_root, "gothic_wind.wesl", &mut next_id)?;

    let base_mesh = [
        "MESH_PIPELINE",
        "VERTEX_POSITIONS",
        "VERTEX_UVS",
        "VERTEX_UVS_A",
        "VERTEX_UVS_B",
        "VERTEX_COLORS",
        "VERTEX_OUTPUT_INSTANCE_INDEX",
    ];
    let mut forward = base_mesh.to_vec();
    forward.extend([
        "VERTEX_NORMALS",
        "VERTEX_TANGENTS",
        "VISIBILITY_RANGE_DITHER",
    ]);

    let prepass_base = [
        "PREPASS_PIPELINE",
        "VIEW_PROJECTION_PERSPECTIVE",
        "MAY_DISCARD",
        "VERTEX_POSITIONS",
        "VERTEX_UVS",
        "VERTEX_UVS_A",
        "VERTEX_COLORS",
        "VERTEX_OUTPUT_INSTANCE_INDEX",
    ];
    // GR_NOWIND still receives VERTEX_NORMALS from mesh layout specialization, even though
    // it does not emit a normal-prepass output. This is the historic failing vertex variant.
    let mut prepass_nowind = prepass_base.to_vec();
    prepass_nowind.push("VERTEX_NORMALS");

    // GothicMaterial::specialize adds the normal input to depth/shadow wind variants and pushes
    // these two defs together when the mesh has a normal attribute.
    let mut prepass_wind = prepass_base.to_vec();
    prepass_wind.extend([
        "NORMAL_PREPASS_OR_DEFERRED_PREPASS",
        "VERTEX_NORMALS",
        "VISIBILITY_RANGE_DITHER",
    ]);

    let mut prepass_compressed = prepass_wind.clone();
    prepass_compressed.extend([
        "VERTEX_NORMALS_COMPRESSED",
        "MOTION_VECTOR_PREPASS",
        "MOTION_VECTOR_PREPASS_OR_DEFERRED_PREPASS",
        "PREPASS_FRAGMENT",
        "VERTEX_UVS_A_COMPRESSED",
    ]);

    let mut prepass_motion = prepass_wind.clone();
    prepass_motion.extend([
        "MOTION_VECTOR_PREPASS",
        "MOTION_VECTOR_PREPASS_OR_DEFERRED_PREPASS",
        "PREPASS_FRAGMENT",
    ]);

    let mut shadow = prepass_wind.clone();
    shadow.retain(|definition| *definition != "PREPASS_FRAGMENT");

    let mut normal_prepass_without_mesh_normal = prepass_base.to_vec();
    normal_prepass_without_mesh_normal.extend([
        "NORMAL_PREPASS_OR_DEFERRED_PREPASS",
        "NORMAL_PREPASS",
        "PREPASS_FRAGMENT",
        "UNCLIPPED_DEPTH_ORTHO_EMULATION",
    ]);

    let mut normal_prepass = prepass_base.to_vec();
    normal_prepass.extend([
        "NORMAL_PREPASS_OR_DEFERRED_PREPASS",
        "NORMAL_PREPASS",
        "VERTEX_NORMALS",
        "PREPASS_FRAGMENT",
    ]);

    let scenarios = [
        ("forward/color", vertex, forward),
        (
            "prepass/GR_NOWIND-shadow-alpha",
            prepass,
            prepass_nowind.clone(),
        ),
        ("prepass/wind-depth-shadow", prepass, prepass_wind),
        ("prepass/shadow-fragment", prepass, shadow),
        ("prepass/compressed-normal", prepass, prepass_compressed),
        ("prepass/motion-vector", prepass, prepass_motion),
        ("prepass/normal", prepass, normal_prepass),
        (
            "prepass/normal-without-mesh-normal",
            prepass,
            normal_prepass_without_mesh_normal,
        ),
    ];

    println!("registered {bevy_module_count} Bevy WESL modules");
    for (index, (name, shader_id, defs)) in scenarios.into_iter().enumerate() {
        let defs = pipeline_defs(&defs);
        cache
            .get(index, shader_id, &defs)
            .map_err(|error| format!("{name}: {error}"))?;
        println!("ok {name}");
    }

    // Prove the checker catches the bug this tool was added to prevent: WESL's @if without
    // braces guards only the next statement, so this mutation leaves world_normal unwritten
    // under GR_NOWIND's VertexOutput variant. This is an expected rejection, not a pipeline.
    let mut fixture_source = fs::read_to_string(shader_root.join("gothic_prepass.wesl"))?;
    let guard_start = fixture_source
        .find("@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS) {")
        .ok_or("normal guard fixture anchor missing")?;
    let guard_end = fixture_source[guard_start..]
        .find("    var wp")
        .map(|offset| guard_start + offset)
        .ok_or("normal guard fixture end missing")?;
    fixture_source.replace_range(
        guard_start..guard_end,
        "@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)\n    wnormal = vertex.normal;\n    out.world_normal = mesh_functions::mesh_normal_local_to_world(\n        vertex.normal,\n        vertex_no_morph.instance_index\n    );\n\n",
    );
    let fixture_id = register_shader(
        &mut cache,
        Shader::from_wesl(
            fixture_source,
            "embedded://gothic_reforged/walk/shaders/fixture_unbraced_prepass.wesl",
        ),
        &mut next_id,
    );
    match cache.get(scenarios_len(), fixture_id, &pipeline_defs(&prepass_nowind)) {
        Err(error) if error.to_string().contains("world_normal") => {
            println!("ok fixture/unbraced-world-normal-rejected: {error}")
        }
        Err(error) => return Err(format!("fixture failed for another reason: {error}").into()),
        Ok(_) => return Err("unbraced world_normal fixture unexpectedly compiled".into()),
    }
    Ok(())
}

fn scenarios_len() -> usize {
    8
}

fn pipeline_defs(flags: &[&'static str]) -> Vec<ShaderDefVal> {
    let mut defs: Vec<_> = flags.iter().copied().map(ShaderDefVal::from).collect();
    // Mirrors PipelineCache's default desktop limits and PrepassPipeline's material group.
    // The numeric values become Bevy's `constants` WESL module entries.
    defs.extend([
        ShaderDefVal::UInt("AVAILABLE_STORAGE_BUFFER_BINDINGS".into(), 8),
        ShaderDefVal::Bool("AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(), true),
        ShaderDefVal::Bool("AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6".into(), true),
        ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3),
    ]);
    defs
}

fn validate_wgsl(
    _: &(),
    source: ShaderCacheSource<'_>,
    _: &ValidateShader,
) -> Result<(), ShaderCacheError> {
    let ShaderCacheSource::Wgsl(source) = source else {
        return Err(ShaderCacheError::ProcessShaderError(
            "expected WESL to produce WGSL".to_owned(),
        ));
    };
    let module = naga::front::wgsl::parse_str(&source)
        .map_err(|error| ShaderCacheError::ProcessShaderError(format!("WGSL parse: {error:?}")))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|error| ShaderCacheError::ProcessShaderError(format!("WGSL validation: {error:?}")))?;
    Ok(())
}

fn register_game_shader(
    cache: &mut ShaderCache<(), ()>,
    shader_root: &Path,
    filename: &str,
    next_id: &mut u128,
) -> AnyResult<AssetId<Shader>> {
    let source = fs::read_to_string(shader_root.join(filename))?;
    let path = format!("embedded://gothic_reforged/walk/shaders/{filename}");
    Ok(register_shader(
        cache,
        Shader::from_wesl(source, path),
        next_id,
    ))
}

fn register_bevy_modules(
    cache: &mut ShaderCache<(), ()>,
    registry_src: &Path,
    next_id: &mut u128,
) -> AnyResult<usize> {
    let mut modules = Vec::new();
    let mut count = 0;
    for registry in fs::read_dir(registry_src)? {
        let registry = registry?.path();
        if !registry.is_dir() {
            continue;
        }
        for crate_entry in fs::read_dir(registry)? {
            let crate_dir = crate_entry?.path();
            let Some(crate_name) = crate_dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(crate_name) = crate_name.strip_suffix("-0.20.0") else {
                continue;
            };
            if !crate_name.starts_with("bevy_") {
                continue;
            }
            let src_dir = crate_dir.join("src");
            if !src_dir.is_dir() {
                continue;
            }
            collect_wesl(&src_dir, &mut modules)?;
            for path in modules.drain(..) {
                let relative = path.strip_prefix(&src_dir)?;
                let embedded = format!(
                    "embedded://{crate_name}/{}",
                    relative.to_string_lossy().replace('\\', "/")
                );
                let source = fs::read_to_string(&path)?;
                register_shader(cache, Shader::from_wesl(source, embedded), next_id);
                count += 1;
            }
        }
    }
    Ok(count)
}

fn collect_wesl(directory: &Path, modules: &mut Vec<PathBuf>) -> AnyResult<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_wesl(&path, modules)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "wesl")
        {
            modules.push(path);
        }
    }
    Ok(())
}

fn register_shader(
    cache: &mut ShaderCache<(), ()>,
    shader: Shader,
    next_id: &mut u128,
) -> AssetId<Shader> {
    let id = AssetId::Uuid {
        uuid: Uuid::from_u128(*next_id),
    };
    *next_id += 1;
    cache.set_shader(id, shader);
    id
}

fn arguments() -> AnyResult<(PathBuf, PathBuf)> {
    let mut repo = None;
    let mut registry_src = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--repo") => repo = args.next().map(PathBuf::from),
            Some("--registry-src") => registry_src = args.next().map(PathBuf::from),
            Some("--help" | "-h") => {
                println!("Usage: gothic-shader-check [--repo PATH] [--registry-src PATH]");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {}", argument.to_string_lossy()).into()),
        }
    }

    let repo = repo.unwrap_or(env::current_dir()?);
    let repo = find_repo_root(&repo).ok_or_else(|| {
        format!(
            "could not find apps/game/src/walk/shaders below {}",
            repo.display()
        )
    })?;
    let registry_src = registry_src.unwrap_or(discover_registry_src()?);
    Ok((repo, registry_src))
}

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|path| {
        path.join("apps/game/src/walk/shaders/gothic_prepass.wesl")
            .is_file()
            .then(|| path.to_path_buf())
    })
}

fn discover_registry_src() -> AnyResult<PathBuf> {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|path| PathBuf::from(path).join(".cargo")))
        .or_else(|| env::var_os("HOME").map(|path| PathBuf::from(path).join(".cargo")))
        .ok_or("set CARGO_HOME or pass --registry-src")?;
    let registry_src = cargo_home.join("registry/src");
    if registry_src.is_dir() {
        Ok(registry_src)
    } else {
        Err(format!(
            "Cargo registry source directory not found: {}",
            registry_src.display()
        )
        .into())
    }
}
