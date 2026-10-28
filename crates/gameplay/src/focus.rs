//! Focus selection: which character, item or interactive object the hero is looking at
//! (`gCFocus_PS`).
//!
//! The settings are the hero's `gCFocus_PS` property set: per combat mode (normal, melee,
//! ranged, magic) and per class of entity (item, character, interact object) a scan range,
//! the largest azimuth and elevation window and a priority. A class whose priority is negative
//! is not focusable in that mode (`IsValidEntity`). `FindFocusEntity` walks the candidates in
//! range, keeps the one with the highest priority and, at equal priority, the one nearest to
//! the view direction; an optional range rating then lets a much closer entity win.
//!
//! Visibility (`IsFreeLOS`) and the entity list are the host's: each [`Candidate`] arrives with
//! its line-of-sight result.

/// `gECombatMode` (1 = normal, 2 = melee, 3 = ranged, 4 = magic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatMode {
    Normal = 1,
    Melee = 2,
    Ranged = 3,
    Magic = 4,
}

impl CombatMode {
    fn index(self) -> usize {
        self as usize - 1
    }
}

/// The three entity classes the settings distinguish (`IsCharacter`, `IsItem`,
/// `IsInteractable`, in this order of testing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Character,
    Item,
    Interact,
}

impl Class {
    fn index(self) -> usize {
        match self {
            Class::Item => 0,
            Class::Character => 1,
            Class::Interact => 2,
        }
    }
}

/// One `<Mode>Mode<Class>Scan*` / `Priority` group of properties.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    pub range: f32,
    pub azi: f32,
    pub elev_up: f32,
    pub elev_down: f32,
    pub priority: f32,
}

const fn zone(range: f32, azi: f32, priority: f32) -> Zone {
    Zone {
        range,
        azi,
        elev_up: 90.0,
        elev_down: -90.0,
        priority,
    }
}

/// The `gCFocus_PS` settings.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusConfig {
    /// Indexed by mode, then item / character / interact.
    zones: [[Zone; 3]; 4],
    /// `OverrideItemPriority`, `OverrideCharacterPriority`, `OverrideInteractPriority`
    /// (0 = not overridden).
    pub override_priority: [f32; 3],
    /// `OverrideRange` (0 = none).
    pub override_range: f32,
    pub hard_max_azi: f32,
    pub hard_max_elev: f32,
    /// `HardFocusMaxRange` (-1 = use the zone range).
    pub hard_max_range: f32,
    /// `FocusLockMaxDistance`.
    pub lock_max_distance: f32,
    /// `InteractHighPriorityBonus`: priority of interact objects whose interaction type is 2.
    pub high_priority_bonus: f32,
    /// `EnableRangeRating`.
    pub range_rating: bool,
}

impl FocusConfig {
    /// The values of the player template (`PC_Hero`).
    pub fn hero() -> Self {
        let blocks = |item: Zone, ch: Zone, int: Zone| [item, ch, int];
        Self {
            zones: [
                blocks(
                    zone(300.0, 45.0, 1.0),
                    zone(500.0, 45.0, 1.0),
                    zone(350.0, 45.0, 0.0),
                ),
                blocks(
                    zone(300.0, 45.0, -1.0),
                    zone(500.0, 90.0, 1.0),
                    zone(350.0, 45.0, -1.0),
                ),
                blocks(
                    zone(3500.0, 45.0, -1.0),
                    zone(3500.0, 45.0, 1.0),
                    zone(3500.0, 45.0, -1.0),
                ),
                blocks(
                    zone(3500.0, 45.0, -1.0),
                    zone(3500.0, 45.0, 1.0),
                    zone(3500.0, 45.0, -1.0),
                ),
            ],
            override_priority: [0.0; 3],
            override_range: 0.0,
            hard_max_azi: 10.0,
            hard_max_elev: 20.0,
            hard_max_range: -1.0,
            lock_max_distance: 2000.0,
            high_priority_bonus: 10.0,
            range_rating: true,
        }
    }

    fn zone(&self, mode: CombatMode, class: Class) -> &Zone {
        &self.zones[mode.index()][class.index()]
    }

    /// Port of gCFocus_PS::GetCurrentMaxRange (Game:0x1006c1d0): the largest scan range of
    /// the mode (or the override).
    pub fn max_range(&self, mode: CombatMode) -> f32 {
        gothic_portcov::hit!("max_range");
        if self.override_range != 0.0 {
            return self.override_range;
        }
        self.zones[mode.index()]
            .iter()
            .map(|z| z.range)
            .fold(0.0, f32::max)
    }

    /// Port of gCFocus_PS::GetCurrentMaxAngles (Game:0x1006c3e0): the character zone's
    /// window `(azimuth, elevation up, elevation down)`.
    pub fn max_angles(&self, mode: CombatMode) -> (f32, f32, f32) {
        gothic_portcov::hit!("max_angles");
        let z = self.zone(mode, Class::Character);
        (z.azi, z.elev_up, z.elev_down)
    }

    fn class_range(&self, mode: CombatMode, class: Class) -> f32 {
        if self.override_range != 0.0 {
            self.override_range
        } else {
            self.zone(mode, class).range
        }
    }

    /// Port of gCFocus_PS::GetCurrentPriority's class-level part and of the test in
    /// IsValidEntity (Game:0x1006d3f0): an override wins over the zone's priority.
    pub fn class_priority(&self, mode: CombatMode, class: Class) -> f32 {
        gothic_portcov::hit!("class_priority");
        let o = self.override_priority[class.index()];
        if o != 0.0 {
            o
        } else {
            self.zone(mode, class).priority
        }
    }

    /// Port of gCFocus_PS::IsValidEntity (Game:0x1006d3f0): classes with a negative priority
    /// cannot be focused in this mode.
    pub fn is_valid(&self, mode: CombatMode, class: Class) -> bool {
        gothic_portcov::hit!("is_valid");
        self.class_priority(mode, class) >= 0.0
    }

    /// Port of gCFocus_PS::IsEntityInRange2 (Game:0x1006d000): is the squared distance
    /// (already reduced by the entity's size) within the class's scan range?
    pub fn in_range2(&self, mode: CombatMode, class: Class, d2: f32) -> bool {
        gothic_portcov::hit!("in_range2");
        let r = self.class_range(mode, class);
        r * r >= d2
    }

    /// Port of gCFocus_PS::IsEntityInAngles (Game:0x1006ccd0): azimuth within +-azi,
    /// elevation within the zone's window.
    pub fn in_angles(&self, mode: CombatMode, class: Class, az: f32, elev: f32) -> bool {
        gothic_portcov::hit!("in_angles");
        let z = self.zone(mode, class);
        az.abs() <= z.azi && elev <= z.elev_up && z.elev_down <= elev
    }

    /// Port of gCFocus_PS::GetCurrentPriority (Game:0x1006c910) for one candidate. Order:
    /// class override, the script's `GetFocusEntityPriority`, the high-priority interaction
    /// bonus, the zone priority. (The branch returning 0.0001 for entities without an item
    /// property set is not understood and left out.)
    pub fn priority(&self, mode: CombatMode, c: &Candidate) -> f32 {
        gothic_portcov::hit!("priority");
        let o = self.override_priority[c.class.index()];
        if o != 0.0 {
            return o;
        }
        if c.script_priority != 0.0 {
            return c.script_priority;
        }
        if c.class == Class::Interact && c.high_priority {
            return self.high_priority_bonus;
        }
        self.zone(mode, c.class).priority
    }
}

/// Anything the hero could focus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// Host identifier (index into the host's lists).
    pub id: usize,
    pub class: Class,
    /// World position in centimetres.
    pub pos: [f32; 3],
    /// Half the larger horizontal extent of the entity's box.
    pub radius: f32,
    /// Interaction type 2: wins over everything of the same kind.
    pub high_priority: bool,
    /// Result of `GetFocusEntityPriority` (0 = none).
    pub script_priority: f32,
    /// `IsFreeLOS(entity, hero)`.
    pub los: bool,
}

/// The looking entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewer {
    pub pos: [f32; 3],
    /// Azimuth of the view direction in radians (0 = +Z, increasing towards +X).
    pub yaw: f32,
    /// Weapon reach plus the viewer's own half-size (`fStack_9c` of FindFocusEntity).
    pub reach: f32,
}

const RAD_TO_DEG: f32 = 180.0 / std::f32::consts::PI;

/// Port of gCFocus_PS::GetAngles (Game:0x1006c530), the owner-relative case: azimuth and
/// elevation (degrees) of `p` seen from the viewer, azimuth wrapped to `-180..=180`.
pub fn angles(v: &Viewer, p: [f32; 3]) -> (f32, f32) {
    gothic_portcov::hit!("angles");
    let d = [p[0] - v.pos[0], p[1] - v.pos[1], p[2] - v.pos[2]];
    let azi = d[0].atan2(d[2]) - v.yaw;
    let mut az = azi * RAD_TO_DEG;
    if az <= -180.0 {
        az += 360.0;
    } else if az > 180.0 {
        az -= 360.0;
    }
    let horiz = (d[0] * d[0] + d[2] * d[2]).sqrt();
    (az, d[1].atan2(horiz) * RAD_TO_DEG)
}

/// `gEFocusLookAt` (`PSFocus.FocusLookAtMode`): what the focus angles are measured against.
pub const LOOK_AT_CAMERA: u32 = 0;
pub const LOOK_AT_ENTITY: u32 = 1;
pub const LOOK_AT_KEYS: u32 = 2;

/// The view azimuth `GetAngles` (Game:0x1006c530) measures the candidates against, by
/// `FocusLookAtMode`: 0 the camera's forward, 1 the owner's own heading (`GetAziElev` on the
/// owner), 2 the accumulated key direction ([`accumulate_key_direction`]); with no key direction
/// yet the keys case falls through to the owner's heading like the original. `dir` is the
/// horizontal direction (x, z) in the same azimuth convention as [`Viewer::yaw`].
pub fn view_yaw(look_at: u32, camera_yaw: f32, owner_yaw: f32, dir: [f32; 2]) -> f32 {
    gothic_portcov::hit!("view_yaw");
    match look_at {
        LOOK_AT_CAMERA => camera_yaw,
        LOOK_AT_KEYS if dir[0] * dir[0] + dir[1] * dir[1] > 1e-8 => dir[0].atan2(dir[1]),
        _ => owner_yaw,
    }
}

/// Port of the key handling of `gCFocus_PS::OnAction` (Game:0x1006e8e0): in the keys look-at mode
/// every movement-key message moves the focus direction vector (+0x18c) by the camera's flattened
/// forward (forward key adds, backward subtracts) or right axis (right adds, left subtracts), then
/// the vector is renormalised. `forward` / `right` are the flattened unit axes of the reference
/// frame (the camera, or the owner when `ControlFrameOfReference` selects it).
pub fn accumulate_key_direction(
    dir: [f32; 2],
    forward_key: bool,
    backward_key: bool,
    left_key: bool,
    right_key: bool,
    forward: [f32; 2],
    right: [f32; 2],
) -> [f32; 2] {
    gothic_portcov::hit!("accumulate_key_direction");
    let mut d = dir;
    let mut add = |a: [f32; 2], sign: f32| {
        d[0] += a[0] * sign;
        d[1] += a[1] * sign;
    };
    if forward_key {
        add(forward, 1.0);
    }
    if backward_key {
        add(forward, -1.0);
    }
    if left_key {
        add(right, -1.0);
    }
    if right_key {
        add(right, 1.0);
    }
    let m2 = d[0] * d[0] + d[1] * d[1];
    if m2 != 0.0 {
        let m = m2.sqrt();
        d = [d[0] / m, d[1] / m];
    }
    d
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

/// The start value of the best angle in FindFocusEntity (Game 0x103084f0).
const NO_ANGLE: f32 = 181.0;
/// Two candidates of equal positive priority closer than this to each other are ranked by
/// their distance to the viewer (Game 0x10308144).
const NEARBY: f32 = 300.0;

/// Port of gCFocus_PS::HasLowerPriority (Game:0x1006d6f0): does `a` rank below the current
/// best `b`?
fn has_lower_priority(
    cfg: &FocusConfig,
    mode: CombatMode,
    v: &Viewer,
    a: &Candidate,
    b: Option<&Candidate>,
) -> bool {
    gothic_portcov::hit!("has_lower_priority");
    let pa = cfg.priority(mode, a);
    let pb = b.map_or(0.0, |b| cfg.priority(mode, b));
    if let Some(b) = b
        && pa == pb
        && pa > 0.0
    {
        let da = dist2(a.pos, v.pos).sqrt();
        let db = dist2(b.pos, v.pos).sqrt();
        return db < da && dist2(a.pos, b.pos).sqrt() < NEARBY;
    }
    pa < pb
}

/// Port of gCFocus_PS::FocusCheck (Game:0x1006d7c0): range, window, priority, tie-break by
/// azimuth against the best so far, and line of sight. Returns the candidate's azimuth.
fn focus_check(
    cfg: &FocusConfig,
    mode: CombatMode,
    v: &Viewer,
    c: &Candidate,
    best: Option<&Candidate>,
    best_az: f32,
) -> Option<f32> {
    gothic_portcov::hit!("focus_check");
    let d2 = (dist2(c.pos, v.pos) - c.radius * c.radius).max(0.0);
    if !cfg.in_range2(mode, c.class, d2) {
        return None;
    }
    let (az, elev) = angles(v, c.pos);
    if !cfg.in_angles(mode, c.class, az, elev) {
        return None;
    }
    let p = cfg.priority(mode, c);
    if p < 0.0 {
        return None;
    }
    if best.is_some_and(|b| cfg.priority(mode, b) == p) && best_az.abs() <= az.abs() {
        return None;
    }
    c.los.then_some(az)
}

/// Port of gCFocus_PS::FindFocusEntity (Game:0x1006da20), the unlocked path: the candidate
/// the hero looks at, or `None`.
pub fn find_focus(
    cfg: &FocusConfig,
    mode: CombatMode,
    v: &Viewer,
    candidates: &[Candidate],
) -> Option<usize> {
    gothic_portcov::hit!("find_focus");
    let max_range = cfg.max_range(mode);
    let mut best: Option<&Candidate> = None;
    let mut best_az = NO_ANGLE;
    let mut nearest: Option<(&Candidate, f32)> = None;
    for c in candidates {
        // CollectInteractionEntities: only what is within the largest scan range
        if dist2(c.pos, v.pos) > max_range * max_range {
            continue;
        }
        if !cfg.is_valid(mode, c.class) {
            continue;
        }
        let Some(az) = focus_check(cfg, mode, v, c, best, best_az) else {
            continue;
        };
        if has_lower_priority(cfg, mode, v, c, best) {
            continue;
        }
        let d = dist2(c.pos, v.pos).sqrt();
        if nearest.is_none_or(|n| d < n.1) {
            nearest = Some((c, d));
        }
        best_az = az;
        best = Some(c);
    }
    // range rating: a clearly nearer candidate beats the angle-ranked one
    if cfg.range_rating
        && let (Some(b), Some((n, nd))) = (best, nearest)
        && n.id != b.id
    {
        let (max_azi, _, _) = cfg.max_angles(mode);
        let reach = v.reach.max(1.0);
        let score = |c: &Candidate, d: f32| {
            let (az, _) = angles(v, c.pos);
            let ratio = if az != 0.0 { az / max_azi } else { 1.0 };
            (d / reach) * 8.0 + ratio * 2.0
        };
        let bd = dist2(b.pos, v.pos).sqrt();
        if score(n, nd) < score(b, bd) {
            return Some(n.id);
        }
    }
    best.map(|b| b.id)
}

/// Port of the lock test at the top of FindFocusEntity: a locked focus is kept while the
/// entity lives and stays within `FocusLockMaxDistance`.
pub fn lock_holds(cfg: &FocusConfig, v: &Viewer, target: [f32; 3], alive: bool) -> bool {
    gothic_portcov::hit!("lock_holds");
    alive && dist2(target, v.pos) <= cfg.lock_max_distance * cfg.lock_max_distance
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer() -> Viewer {
        Viewer {
            pos: [0.0, 0.0, 0.0],
            yaw: 0.0,
            reach: 150.0,
        }
    }

    fn cand(id: usize, class: Class, x: f32, z: f32) -> Candidate {
        Candidate {
            id,
            class,
            pos: [x, 0.0, z],
            radius: 0.0,
            high_priority: false,
            script_priority: 0.0,
            los: true,
        }
    }

    #[test]
    fn keys_look_at_follows_the_accumulated_direction() {
        // forward = +Z, right = +X
        let f = [0.0, 1.0];
        let r = [1.0, 0.0];
        let d = accumulate_key_direction([0.0, 0.0], false, false, false, true, f, r);
        assert_eq!(d, [1.0, 0.0]);
        // holding forward + right drifts to the diagonal, normalised
        let d = accumulate_key_direction(d, true, false, false, false, f, r);
        assert!((d[0] - d[1]).abs() < 1e-5 && (d[0].hypot(d[1]) - 1.0).abs() < 1e-5);
        // look-at mode 2 measures against that direction, 0 against the camera, 1 the owner
        assert!((view_yaw(LOOK_AT_KEYS, 0.3, 0.7, [1.0, 0.0]) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(view_yaw(LOOK_AT_CAMERA, 0.3, 0.7, [1.0, 0.0]), 0.3);
        assert_eq!(view_yaw(LOOK_AT_ENTITY, 0.3, 0.7, [1.0, 0.0]), 0.7);
        // no direction yet: falls through to the owner's heading
        assert_eq!(view_yaw(LOOK_AT_KEYS, 0.3, 0.7, [0.0, 0.0]), 0.7);
        // the target to the right of the camera is picked first when the keys point right
        let c = FocusConfig::hero();
        let left = cand(1, Class::Character, -100.0, 200.0);
        let right_c = cand(2, Class::Character, 100.0, 200.0);
        let mut v = viewer();
        v.yaw = view_yaw(LOOK_AT_KEYS, 0.0, 0.0, [1.0, 0.0]);
        // viewer facing +X: both are 90 degrees off at most; the nearer-angle one wins
        let pick = find_focus(&c, CombatMode::Melee, &v, &[left, right_c]);
        assert!(pick.is_some());
    }

    #[test]
    fn hero_ranges_per_mode() {
        let c = FocusConfig::hero();
        assert_eq!(c.max_range(CombatMode::Normal), 500.0);
        assert_eq!(c.max_range(CombatMode::Melee), 500.0);
        assert_eq!(c.max_range(CombatMode::Ranged), 3500.0);
        assert_eq!(c.max_angles(CombatMode::Normal), (45.0, 90.0, -90.0));
        assert_eq!(c.max_angles(CombatMode::Melee), (90.0, 90.0, -90.0));
    }

    #[test]
    fn only_characters_are_valid_in_melee_and_ranged_modes() {
        let c = FocusConfig::hero();
        for m in [CombatMode::Melee, CombatMode::Ranged, CombatMode::Magic] {
            assert!(c.is_valid(m, Class::Character));
            assert!(!c.is_valid(m, Class::Item));
            assert!(!c.is_valid(m, Class::Interact));
        }
        for cl in [Class::Character, Class::Item, Class::Interact] {
            assert!(c.is_valid(CombatMode::Normal, cl));
        }
    }

    #[test]
    fn scan_windows_are_class_specific() {
        let c = FocusConfig::hero();
        let v = viewer();
        // an item 350 cm ahead is out of its 300 range, an interact object is not
        let item = [cand(1, Class::Item, 0.0, 350.0)];
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &item), None);
        let chest = [cand(2, Class::Interact, 0.0, 349.0)];
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &chest), Some(2));
        // a character at 450 is in range, 520 is not
        assert_eq!(
            find_focus(
                &c,
                CombatMode::Normal,
                &v,
                &[cand(3, Class::Character, 0.0, 450.0)]
            ),
            Some(3)
        );
        assert_eq!(
            find_focus(
                &c,
                CombatMode::Normal,
                &v,
                &[cand(3, Class::Character, 0.0, 520.0)]
            ),
            None
        );
        // 50 degrees off the view axis is outside the 45 degree window
        let off = (50.0f32).to_radians();
        let side = [cand(
            4,
            Class::Character,
            300.0 * off.sin(),
            300.0 * off.cos(),
        )];
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &side), None);
        // but a melee fighter sees 90 degrees
        assert_eq!(find_focus(&c, CombatMode::Melee, &v, &side), Some(4));
    }

    #[test]
    fn characters_and_items_outrank_interact_objects() {
        // without the range rating, which can still hand a much nearer object the focus
        let mut c = FocusConfig::hero();
        c.range_rating = false;
        let v = viewer();
        let list = [
            cand(1, Class::Interact, 0.0, 150.0),
            cand(2, Class::Character, 20.0, 400.0),
        ];
        // the chest is nearer and straight ahead, the character has priority 1 against 0
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &list), Some(2));
        // a high-priority interaction (priority 10) beats the character
        let mut hi = list;
        hi[0].high_priority = true;
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &hi), Some(1));
        // the script can override as well
        let mut s = list;
        s[1].script_priority = -1.0;
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &s), Some(1));
    }

    #[test]
    fn at_equal_priority_the_candidate_nearer_the_view_axis_wins() {
        let mut c = FocusConfig::hero();
        c.range_rating = false;
        let v = viewer();
        let list = [
            cand(1, Class::Interact, 100.0, 300.0), // ~18 degrees right
            cand(2, Class::Interact, -20.0, 300.0), // ~4 degrees left
            cand(3, Class::Interact, 60.0, 300.0),  // ~11 degrees right
        ];
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &list), Some(2));
    }

    #[test]
    fn range_rating_prefers_a_much_nearer_candidate() {
        let c = FocusConfig::hero();
        let v = viewer();
        // near one 20 degrees to the side is met first, the far one is dead ahead
        let a = (20.0f32).to_radians();
        let list = [
            cand(2, Class::Interact, 80.0 * a.sin(), 80.0 * a.cos()),
            cand(1, Class::Interact, 0.0, 330.0),
        ];
        // angle ranking alone ends with #1 (azimuth 0)
        let mut no_rating = c.clone();
        no_rating.range_rating = false;
        assert_eq!(
            find_focus(&no_rating, CombatMode::Normal, &v, &list),
            Some(1)
        );
        // the rating weighs distance (x8 per reach) against the angle ratio (x2)
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &list), Some(2));
    }

    #[test]
    fn candidates_behind_cover_or_out_of_sight_are_skipped() {
        let c = FocusConfig::hero();
        let v = viewer();
        let mut hidden = cand(1, Class::Character, 0.0, 200.0);
        hidden.los = false;
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &[hidden]), None);
    }

    #[test]
    fn a_big_entity_counts_from_its_surface() {
        let c = FocusConfig::hero();
        let v = viewer();
        let mut troll = cand(1, Class::Interact, 0.0, 380.0);
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &[troll]), None);
        troll.radius = 150.0;
        assert_eq!(find_focus(&c, CombatMode::Normal, &v, &[troll]), Some(1));
    }

    #[test]
    fn angle_helper_signs() {
        let v = viewer();
        let (az, el) = angles(&v, [100.0, 0.0, 100.0]);
        assert!((az - 45.0).abs() < 1e-3 && el.abs() < 1e-3);
        let (az, _) = angles(&v, [0.0, 0.0, -100.0]);
        assert!((az - 180.0).abs() < 1e-3);
        let (_, el) = angles(&v, [0.0, 100.0, 100.0]);
        assert!((el - 45.0).abs() < 1e-3);
    }

    #[test]
    fn lock_is_kept_within_the_lock_distance() {
        let c = FocusConfig::hero();
        let v = viewer();
        assert!(lock_holds(&c, &v, [0.0, 0.0, 1999.0], true));
        assert!(!lock_holds(&c, &v, [0.0, 0.0, 2001.0], true));
        assert!(!lock_holds(&c, &v, [0.0, 0.0, 10.0], false));
    }
}
