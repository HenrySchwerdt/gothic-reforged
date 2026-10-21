//! `eCParticle_PS` emitter parameters, decoded from the entity's property set and tail.
//! Property names and enum values come from the class schema (`class-schemas.json`); the
//! meanings of the fields follow the engine's `Spawn`, `TickParticles`, `CalcFrame`,
//! `CalcColorFactor` and `UpdateVertices` (see `docs/research/ENGINE_PORT_PLAN.md` section 7).
use crate::rng::Rng;
use crate::scale::Curves;
use gothic_formats::props::tails::Tail;
use gothic_formats::propstream::{PropValue, PropertySet};

/// `bCRange1`: a uniformly drawn float interval.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Range1 {
    pub min: f32,
    pub max: f32,
}

impl Range1 {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }
    pub fn sample(&self, rng: &mut Rng) -> f32 {
        self.min + (self.max - self.min) * rng.unit()
    }
}

/// `bCRange3`: a box; each axis is drawn independently.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Range3 {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Range3 {
    pub const fn splat(v: f32) -> Self {
        Self {
            min: [v; 3],
            max: [v; 3],
        }
    }
    pub fn sample(&self, rng: &mut Rng) -> [f32; 3] {
        let mut out = [0.0; 3];
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.min[i] + (self.max[i] - self.min[i]) * rng.unit();
        }
        out
    }
    /// The same box in the x-mirrored space (x -> -x).
    pub fn mirrored_x(&self) -> Self {
        let mut r = *self;
        r.min[0] = -self.max[0];
        r.max[0] = -self.min[0];
        r
    }
}

/// `eETextureDrawStyle` mapped to the engine's blend mode (`ApplyRenderStatesFX`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DrawStyle {
    /// Opaque unless the alpha test is on.
    #[default]
    Regular,
    AlphaBlend,
    Modulated,
    Translucent,
    AlphaModulate,
    Darken,
    /// Additive.
    Brighten,
    Invisible,
}

impl DrawStyle {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::AlphaBlend,
            2 => Self::Modulated,
            3 => Self::Translucent,
            4 => Self::AlphaModulate,
            5 => Self::Darken,
            6 => Self::Brighten,
            7 => Self::Invisible,
            _ => Self::Regular,
        }
    }
}

/// `eEFacingDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Facing {
    #[default]
    Camera,
    AlongMovementFacingCamera,
    SpecifiedNormal,
    AlongMovementFacingNormal,
    PerpendicularToMovement,
}

impl Facing {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::AlongMovementFacingCamera,
            2 => Self::SpecifiedNormal,
            3 => Self::AlongMovementFacingNormal,
            4 => Self::PerpendicularToMovement,
            _ => Self::Camera,
        }
    }
}

/// `eEVelocityDirectionFrom`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VelocityFrom {
    #[default]
    None,
    /// Away from the owner: the direction is the inverted start position.
    StartPositionAndOwner,
    /// From the owner: the direction is the start position.
    OwnerAndStartPosition,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmitterParams {
    pub max_particles: usize,
    pub particles_per_second: f32,
    /// Dead particles leave the list at once (their slot is reused).
    pub respawn_dead: bool,
    /// Fill the whole buffer at once instead of the rate.
    pub automatic_spawning: bool,
    pub auto_reset: bool,
    pub auto_kill: bool,
    pub enabled: bool,
    pub lifetime: Range1,
    pub initial_age: Range1,
    pub initial_delay: Range1,
    pub ticks_per_second: f32,
    pub relative_warmup_time: f32,
    pub warmup_ticks_per_second: f32,
    /// 0 box, 1 sphere.
    pub start_shape: i32,
    pub start_box: Range3,
    pub start_sphere_radius: Range1,
    pub start_offset: [f32; 3],
    /// 1: particles live in the emitter's frame (`Relative`), 0: in the world (`Independent`).
    pub relative: bool,
    pub start_velocity: Range3,
    pub velocity_loss: Range3,
    pub acceleration: [f32; 3],
    pub max_velocity: [f32; 3],
    pub use_velocity_scale: bool,
    pub velocity_scale_repeats: f32,
    pub velocity_from: VelocityFrom,
    pub spin: bool,
    pub start_spin: Range3,
    pub spins_per_second: Range3,
    /// Probability that a spin axis flips its sign (`SpinDirection`).
    pub spin_direction: f32,
    pub facing: Facing,
    pub projection_normal: [f32; 3],
    pub spin_pivot: [f32; 3],
    /// `UseRotationFrom == Entity`: offsets and velocities are rotated by the entity matrix.
    pub rotate_with_entity: bool,
    pub use_revolution: bool,
    pub revolutions_per_second: Range3,
    pub revolution_center: Range3,
    pub use_revolution_scale: bool,
    pub revolution_scale_repeats: f32,
    pub uniform_size: bool,
    pub start_size: Range3,
    pub use_size_scale: bool,
    pub size_scale_repeats: f32,
    pub stretch_to_emitter: bool,
    pub texture: String,
    pub draw_style: DrawStyle,
    pub u_subdivisions: i32,
    pub v_subdivisions: i32,
    pub subdivision_start: i32,
    pub subdivision_end: i32,
    pub random_subdivision: bool,
    pub blend_subdivisions: bool,
    pub row_major: bool,
    pub use_subdivision_scale: bool,
    pub subdivision_scale_repeats: f32,
    pub fade_in: bool,
    pub fade_out: bool,
    pub fade_in_end: f32,
    pub fade_out_start: f32,
    /// rgb multiplier at the start of the fade-in; alpha is the amount removed.
    pub fade_in_factor: [f32; 4],
    pub fade_out_factor: [f32; 4],
    pub use_color_scale: bool,
    pub color_scale_repeats: f32,
    pub disable_fogging: bool,
    pub alpha_test: bool,
    pub alpha_reference: u8,
    pub depth_test: bool,
    pub depth_write: bool,
    pub min_squared_velocity: f32,
    pub curves: Curves,
}

impl Default for EmitterParams {
    fn default() -> Self {
        Self {
            max_particles: 100,
            particles_per_second: 10.0,
            respawn_dead: true,
            automatic_spawning: false,
            auto_reset: false,
            auto_kill: false,
            enabled: true,
            lifetime: Range1::new(1.0, 1.0),
            initial_age: Range1::default(),
            initial_delay: Range1::default(),
            ticks_per_second: 25.0,
            relative_warmup_time: 0.0,
            warmup_ticks_per_second: 1.0,
            start_shape: 0,
            start_box: Range3::default(),
            start_sphere_radius: Range1::default(),
            start_offset: [0.0; 3],
            relative: false,
            start_velocity: Range3::default(),
            velocity_loss: Range3::default(),
            acceleration: [0.0; 3],
            max_velocity: [0.0; 3],
            use_velocity_scale: false,
            velocity_scale_repeats: 0.0,
            velocity_from: VelocityFrom::None,
            spin: false,
            start_spin: Range3::default(),
            spins_per_second: Range3::default(),
            spin_direction: 0.5,
            facing: Facing::Camera,
            projection_normal: [0.0, 1.0, 0.0],
            spin_pivot: [0.0; 3],
            rotate_with_entity: false,
            use_revolution: false,
            revolutions_per_second: Range3::default(),
            revolution_center: Range3::default(),
            use_revolution_scale: false,
            revolution_scale_repeats: 0.0,
            uniform_size: true,
            start_size: Range3::splat(10.0),
            use_size_scale: false,
            size_scale_repeats: 0.0,
            stretch_to_emitter: false,
            texture: String::new(),
            draw_style: DrawStyle::Regular,
            u_subdivisions: 0,
            v_subdivisions: 0,
            subdivision_start: 0,
            subdivision_end: 0,
            random_subdivision: false,
            blend_subdivisions: false,
            row_major: false,
            use_subdivision_scale: false,
            subdivision_scale_repeats: 0.0,
            fade_in: false,
            fade_out: false,
            fade_in_end: 0.5,
            fade_out_start: 0.5,
            fade_in_factor: [1.0; 4],
            fade_out_factor: [1.0; 4],
            use_color_scale: false,
            color_scale_repeats: 0.0,
            disable_fogging: false,
            alpha_test: false,
            alpha_reference: 127,
            depth_test: true,
            depth_write: false,
            min_squared_velocity: 0.0,
            curves: Curves::default(),
        }
    }
}

fn enum_of(set: &PropertySet, name: &str) -> i32 {
    match set.get(name) {
        Some(PropValue::Enum { value, .. }) => *value,
        _ => 0,
    }
}

fn range1(set: &PropertySet, name: &str, default: Range1) -> Range1 {
    match set.get(name) {
        Some(PropValue::Range1([a, b])) => Range1::new(*a, *b),
        _ => default,
    }
}

fn range3(set: &PropertySet, name: &str, default: Range3) -> Range3 {
    match set.get(name) {
        Some(PropValue::Range3([a, b])) => Range3 { min: *a, max: *b },
        _ => default,
    }
}

fn vec3(set: &PropertySet, name: &str, default: [f32; 3]) -> [f32; 3] {
    match set.get(name) {
        Some(PropValue::Vec3(v)) => *v,
        _ => default,
    }
}

fn rgba(set: &PropertySet, name: &str) -> [f32; 4] {
    match set.get(name) {
        Some(PropValue::ColorRgba(c)) => *c,
        _ => [1.0; 4],
    }
}

fn flag(set: &PropertySet, name: &str, default: bool) -> bool {
    set.boolean(name).unwrap_or(default)
}

fn num(set: &PropertySet, name: &str, default: f32) -> f32 {
    set.float(name).unwrap_or(default)
}

fn int(set: &PropertySet, name: &str, default: i32) -> i32 {
    set.int(name).unwrap_or(default)
}

impl EmitterParams {
    /// Decode an `eCParticle_PS` property set. `None` when the set is another class.
    pub fn from_set(set: &PropertySet) -> Option<Self> {
        if set.class != "eCParticle_PS" {
            return None;
        }
        let d = Self::default();
        let texture = match set.string("Texture") {
            Some(t) if !t.is_empty() => t.to_owned(),
            _ => set.string("Material").unwrap_or_default().to_owned(),
        };
        let alpha_reference = match set.get("AlphaReference") {
            Some(PropValue::Char(c)) => *c,
            _ => d.alpha_reference,
        };
        let curves = match &set.tail {
            Tail::Particle(c) => Curves::from(&**c),
            _ => Curves::default(),
        };
        Some(Self {
            max_particles: int(set, "MaxNumParticles", d.max_particles as i32).clamp(0, 4096)
                as usize,
            particles_per_second: num(set, "ParticlesPerSecond", d.particles_per_second),
            respawn_dead: flag(set, "RespawnDeadParticles", d.respawn_dead),
            automatic_spawning: flag(set, "AutomaticSpawning", d.automatic_spawning),
            auto_reset: flag(set, "AutoReset", d.auto_reset),
            auto_kill: flag(set, "AutoKill", d.auto_kill),
            enabled: flag(set, "Enabled", d.enabled),
            lifetime: range1(set, "Lifetime", d.lifetime),
            initial_age: range1(set, "InitialAge", d.initial_age),
            initial_delay: range1(set, "InitialDelay", d.initial_delay),
            ticks_per_second: num(set, "TicksPerSecond", d.ticks_per_second),
            relative_warmup_time: num(set, "RelativeWarmupTime", d.relative_warmup_time),
            warmup_ticks_per_second: num(set, "WarmupTicksPerSecond", d.warmup_ticks_per_second),
            start_shape: enum_of(set, "StartLocationShape"),
            start_box: range3(set, "StartBoxLocation", d.start_box),
            start_sphere_radius: range1(set, "StartSphereRadius", d.start_sphere_radius),
            start_offset: vec3(set, "StartLocationOffset", d.start_offset),
            relative: enum_of(set, "CoordinateSystem") == 1,
            start_velocity: range3(set, "StartVelocity", d.start_velocity),
            velocity_loss: range3(set, "VelocityLoss", d.velocity_loss),
            acceleration: vec3(set, "Acceleration", d.acceleration),
            max_velocity: vec3(set, "MaxAbsoluteVelocity", d.max_velocity),
            use_velocity_scale: flag(set, "UseVelocityScale", d.use_velocity_scale),
            velocity_scale_repeats: num(set, "VelocityScaleRepeats", 0.0),
            velocity_from: match enum_of(set, "VelocityDirectionFrom") {
                1 => VelocityFrom::StartPositionAndOwner,
                2 => VelocityFrom::OwnerAndStartPosition,
                _ => VelocityFrom::None,
            },
            spin: flag(set, "SpinParticles", d.spin),
            start_spin: range3(set, "StartSpin", d.start_spin),
            spins_per_second: range3(set, "SpinsPerSecond", d.spins_per_second),
            spin_direction: num(set, "SpinDirection", d.spin_direction),
            facing: Facing::from_i32(enum_of(set, "FacingDirection")),
            projection_normal: vec3(set, "ProjectionNormal", d.projection_normal),
            spin_pivot: vec3(set, "RelativeSpinPivot", d.spin_pivot),
            rotate_with_entity: enum_of(set, "UseRotationFrom") == 1,
            use_revolution: flag(set, "UseRevolution", d.use_revolution),
            revolutions_per_second: range3(set, "RevolutionsPerSecond", d.revolutions_per_second),
            revolution_center: range3(set, "RevolutionCenterOffset", d.revolution_center),
            use_revolution_scale: flag(set, "UseRevolutionScale", d.use_revolution_scale),
            revolution_scale_repeats: num(set, "RevolutionScaleRepeats", 0.0),
            uniform_size: flag(set, "UniformSize", d.uniform_size),
            start_size: range3(set, "StartSize", d.start_size),
            use_size_scale: flag(set, "UseSizeScale", d.use_size_scale),
            size_scale_repeats: num(set, "SizeScaleRepeats", 0.0),
            stretch_to_emitter: flag(set, "StretchToEmitter", d.stretch_to_emitter),
            texture,
            draw_style: DrawStyle::from_i32(enum_of(set, "DrawStyle")),
            u_subdivisions: int(set, "NumUSubdivisions", 0).max(0),
            v_subdivisions: int(set, "NumVSubdivisions", 0).max(0),
            subdivision_start: int(set, "SubdivisionStart", 0),
            subdivision_end: int(set, "SubdivisionEnd", 0),
            random_subdivision: flag(set, "RandomSubdivision", d.random_subdivision),
            blend_subdivisions: flag(set, "BlendBetweenSubdivisions", d.blend_subdivisions),
            row_major: flag(set, "SubdivisionRowMajor", d.row_major),
            use_subdivision_scale: flag(set, "UseSubdivisionScale", d.use_subdivision_scale),
            subdivision_scale_repeats: num(set, "SubdivisionScaleRepeats", 0.0),
            fade_in: flag(set, "FadeIn", d.fade_in),
            fade_out: flag(set, "FadeOut", d.fade_out),
            fade_in_end: num(set, "FadeInEndTime", d.fade_in_end),
            fade_out_start: num(set, "FadeOutStartTime", d.fade_out_start),
            fade_in_factor: rgba(set, "FaceInFactor"),
            fade_out_factor: rgba(set, "FaceOutFactor"),
            use_color_scale: flag(set, "UseColorScale", d.use_color_scale),
            color_scale_repeats: num(set, "ColorScaleRepeats", 0.0),
            disable_fogging: flag(set, "DisableFogging", d.disable_fogging),
            alpha_test: flag(set, "AlphaTest", d.alpha_test),
            alpha_reference,
            depth_test: flag(set, "DepthTest", d.depth_test),
            depth_write: flag(set, "DepthWrite", d.depth_write),
            min_squared_velocity: num(set, "MinSquaredVelocity", 0.0),
            curves,
        })
    }

    /// The parameters in the x-mirrored world space the port renders in (`x -> -x`; the engine
    /// is left-handed and the loader mirrors every world position the same way).
    pub fn mirrored_x(&self) -> Self {
        let mut p = self.clone();
        let flip = |v: [f32; 3]| [-v[0], v[1], v[2]];
        p.start_box = self.start_box.mirrored_x();
        p.start_offset = flip(self.start_offset);
        p.start_velocity = self.start_velocity.mirrored_x();
        p.acceleration = flip(self.acceleration);
        p.projection_normal = flip(self.projection_normal);
        p.spin_pivot = flip(self.spin_pivot);
        p.revolution_center = self.revolution_center.mirrored_x();
        p
    }

    /// Longest particle life (s); the effect is finished that long after the last spawn.
    pub fn max_life(&self) -> f32 {
        self.lifetime.max.max(self.lifetime.min)
    }

    /// Number of sub-images in the texture grid.
    pub fn frame_count(&self) -> i32 {
        (self.u_subdivisions + 1) * (self.v_subdivisions + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirroring_swaps_range_bounds() {
        let r = Range3 {
            min: [1.0, 0.0, 0.0],
            max: [3.0, 1.0, 1.0],
        };
        let m = r.mirrored_x();
        assert_eq!(m.min[0], -3.0);
        assert_eq!(m.max[0], -1.0);
        assert_eq!(m.min[1], 0.0);
    }

    #[test]
    fn draw_style_numbers_follow_the_enum() {
        assert_eq!(DrawStyle::from_i32(6), DrawStyle::Brighten);
        assert_eq!(DrawStyle::from_i32(99), DrawStyle::Regular);
        assert_eq!(Facing::from_i32(2), Facing::SpecifiedNormal);
    }
}
