//! The ambient system (`gCAmbientModule`, `gCAmbientSequencer`, `gCAmbientLibrary`; ENGINE_MAP
//! C11, batch E37): random one-shot sounds (birds, crickets, wind) chosen per climate,
//! environment and time of day, and the distance-driven water beds.
//!
//! `Library.pak/g3.ams` holds 28 `gCAmbientSound` rows and 24 `gCAmbientEnvironment` rows
//! (4 climates x 6 environments, row `climate * 6 + environment`).
use crate::mul::MulDocument;
use crate::rng::Rng;

/// `eEAmbientClimate` (names from the game's string table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Climate {
    #[default]
    Mediterranean = 0,
    Arctic = 1,
    Desert = 2,
    Cave = 3,
}

/// `eEAmbientEnvironment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Environment {
    Forest = 0,
    Meadow = 1,
    Plain = 2,
    Mountain = 3,
    Coast = 4,
    Town = 5,
}

/// The environment used when no ambient volume dominates (`UpdateEnvironment` falls back to 2).
pub const DEFAULT_ENVIRONMENT: u8 = Environment::Plain as u8;

/// `eEAmbientDayTime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DayTime {
    Morning = 0,
    Day = 1,
    Evening = 2,
    Night = 3,
}

impl DayTime {
    /// From the in-game hour (0..24): 5-7 morning, 7-18 day, 18-21 evening, else night. The
    /// capture of the 08:00 game start plays the day set, which fixes the end of the morning.
    pub fn from_hour(hour: f32) -> Self {
        match hour.rem_euclid(24.0) {
            h if (5.0..7.0).contains(&h) => Self::Morning,
            h if (7.0..18.0).contains(&h) => Self::Day,
            h if (18.0..21.0).contains(&h) => Self::Evening,
            _ => Self::Night,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AmbientSound {
    pub climate: u8,
    pub environment: u8,
    pub day_time: u8,
    /// Sample as authored (`bird_01.wav`; shipped as Ogg in `Sound.pak`).
    pub sample: String,
    pub volume: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    /// How many times the sound enters the draw list (`FindSounds` adds it `Weight` times).
    pub weight: i32,
    pub duration_ms: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmbientEnvironment {
    pub min_sounds: i32,
    pub max_sounds: i32,
    pub min_delay: f32,
    pub max_delay: f32,
    pub spawn_probability: f32,
}

impl Default for AmbientEnvironment {
    fn default() -> Self {
        Self {
            min_sounds: 1,
            max_sounds: 5,
            min_delay: 0.5,
            max_delay: 2.5,
            spawn_probability: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AmbientLibrary {
    pub sounds: Vec<AmbientSound>,
    pub environments: Vec<AmbientEnvironment>,
}

impl AmbientLibrary {
    pub fn from_document(doc: &MulDocument) -> Self {
        let mut lib = Self::default();
        for o in &doc.objects {
            match o.class.as_str() {
                "gCAmbientSound" => lib.sounds.push(AmbientSound {
                    climate: o.int("Climate").unwrap_or(0) as u8,
                    environment: o.int("Environment").unwrap_or(0) as u8,
                    day_time: o.int("DayTime").unwrap_or(0) as u8,
                    sample: o.string("Sample").unwrap_or_default().to_owned(),
                    volume: o.float("Volume").unwrap_or(1.0),
                    min_distance: o.float("MinDistance").unwrap_or(500.0),
                    max_distance: o.float("MaxDistance").unwrap_or(1.0e6),
                    weight: o.int("Weight").unwrap_or(1),
                    duration_ms: o.int("Duration").unwrap_or(0),
                }),
                "gCAmbientEnvironment" => lib.environments.push(AmbientEnvironment {
                    min_sounds: o.int("MinNumSounds").unwrap_or(1),
                    max_sounds: o.int("MaxNumSounds").unwrap_or(5),
                    min_delay: o.float("MinSpawnDelay").unwrap_or(0.5),
                    max_delay: o.float("MaxSpawnDelay").unwrap_or(2.5),
                    spawn_probability: o.float("SpawnProbability").unwrap_or(1.0),
                }),
                _ => {}
            }
        }
        lib
    }

    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(Self::from_document(&crate::mul::parse(bytes)?))
    }

    /// Port of gCAmbientLibrary::GetEnvironment (Game:0x101b9af0).
    pub fn environment(&self, climate: u8, environment: u8) -> Option<&AmbientEnvironment> {
        if climate >= 4 || environment >= 6 {
            return self.environments.first();
        }
        self.environments
            .get(usize::from(climate) * 6 + usize::from(environment))
    }

    /// Port of gCAmbientSoundList::FindSounds (Game:0x101bea20): sound indices matching the
    /// triple exactly, each repeated `Weight` times.
    pub fn find_sounds(&self, climate: u8, environment: u8, day_time: u8) -> Vec<usize> {
        let mut list = Vec::new();
        for (i, s) in self.sounds.iter().enumerate() {
            if s.climate == climate && s.environment == environment && s.day_time == day_time {
                list.extend(std::iter::repeat_n(i, s.weight.max(0) as usize));
            }
        }
        list
    }

    /// Port of gCAmbientSoundList::SelectSound (Game:0x101bead0): uniform over the draw list.
    pub fn select_sound(&self, list: &[usize], rng: &mut Rng) -> Option<usize> {
        match list.len() {
            0 => None,
            1 => Some(list[0]),
            n => Some(list[rng.below(n)]),
        }
    }
}

/// Port of gCAmbientModule::UpdateEnvironment (Game:0x101b7c00): the environment type whose
/// volume counter is the largest, but only above 4; otherwise [`DEFAULT_ENVIRONMENT`].
pub fn dominant_environment(counters: &[u32; 6]) -> u8 {
    let (best, count) =
        counters.iter().enumerate().fold(
            (0usize, 0u32),
            |acc, (i, &c)| {
                if c > acc.1 { (i, c) } else { acc }
            },
        );
    if count > 4 {
        best as u8
    } else {
        DEFAULT_ENVIRONMENT
    }
}

/// One spawn decision of the sequencer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    /// Index into [`AmbientLibrary::sounds`].
    pub sound: usize,
}

/// Port of gCAmbientSequencer's spawning logic.
#[derive(Debug, Clone)]
pub struct AmbientSequencer {
    pub climate: u8,
    pub environment: u8,
    pub day_time: u8,
    delay: f32,
    rng: Rng,
    /// Delays that were armed, newest last (`Starting delay: x seconds`; bounded).
    pub log: Vec<f32>,
}

impl AmbientSequencer {
    pub fn new(seed: u64) -> Self {
        Self {
            climate: Climate::Mediterranean as u8,
            environment: DEFAULT_ENVIRONMENT,
            day_time: DayTime::Day as u8,
            delay: 0.0,
            rng: Rng::new(seed),
            log: Vec::new(),
        }
    }

    pub fn delay(&self) -> f32 {
        self.delay
    }

    /// Port of gCAmbientSequencer::Process (Game:0x101ba9a0), spawning part. `playing` is the
    /// number of ambient one-shots still sounding. While fewer than `MinNumSounds` play, one is
    /// spawned at once (without re-arming the delay). Otherwise the delay counts down; when it
    /// expires and fewer than `MaxNumSounds` play, `SpawnProbability` decides; the delay is
    /// re-armed with a random value of the environment's range.
    pub fn process(&mut self, lib: &AmbientLibrary, dt: f32, playing: usize) -> Option<Spawn> {
        let env = *lib.environment(self.climate, self.environment)?;
        let playing = playing as i32;
        let mut spawn = false;
        let mut rearm = true;
        if playing < env.min_sounds {
            spawn = true;
            rearm = false;
        } else {
            if self.delay > 0.0 {
                self.delay -= dt;
            }
            if self.delay > 0.0 {
                return None;
            }
            self.delay = 0.0;
            if playing < env.max_sounds && self.rng.unit() <= env.spawn_probability {
                spawn = true;
            }
        }
        let chosen = if spawn { self.pick(lib) } else { None };
        if rearm {
            self.delay = self.rng.range(env.min_delay, env.max_delay);
            if self.log.len() >= 64 {
                self.log.remove(0);
            }
            self.log.push(self.delay);
        }
        chosen.map(|sound| Spawn { sound })
    }

    fn pick(&mut self, lib: &AmbientLibrary) -> Option<usize> {
        let list = lib.find_sounds(self.climate, self.environment, self.day_time);
        lib.select_sound(&list, &mut self.rng)
    }
}

/// Volumes of the three water beds (`UpdateWaterSounds`, Game:0x101bab90).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WaterBeds {
    /// `water_coast_01`: while the ocean is closer than 10 000 cm.
    pub coast: f32,
    /// `water_oceanwaves_01`: while the ocean is closer than 2 000 cm.
    pub waves: f32,
    /// `water_river_01`: while the normalised river distance is at most 1.
    pub river: f32,
}

impl WaterBeds {
    pub const COAST_RANGE_CM: f32 = 10_000.0;
    pub const WAVES_RANGE_CM: f32 = 2_000.0;

    /// Bed volumes from the camera's distance to the ocean (cm) and the normalised river
    /// distance; `None` = no ocean / river.
    pub fn at(ocean_cm: Option<f32>, river: Option<f32>) -> Self {
        let ramp = |d: Option<f32>, range: f32, gain: f32| match d {
            Some(d) if d < range => (1.0 - d.max(0.0) / range) * gain,
            _ => 0.0,
        };
        Self {
            coast: ramp(ocean_cm, Self::COAST_RANGE_CM, 0.3),
            waves: ramp(ocean_cm, Self::WAVES_RANGE_CM, 0.2),
            river: match river {
                Some(v) if v <= 1.0 => ((1.0 - 1.4 * v) * 0.18).clamp(0.0, 1.0),
                _ => 0.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> AmbientLibrary {
        let env = |min, max| AmbientEnvironment {
            min_sounds: min,
            max_sounds: max,
            min_delay: 2.0,
            max_delay: 1.0,
            spawn_probability: 1.0,
        };
        let s = |name: &str, day: u8, weight: i32| AmbientSound {
            climate: 0,
            environment: 2,
            day_time: day,
            sample: name.into(),
            volume: 0.2,
            min_distance: 500.0,
            max_distance: 1.0e6,
            weight,
            duration_ms: 0,
        };
        AmbientLibrary {
            sounds: vec![s("a.wav", 1, 1), s("b.wav", 1, 3), s("owl.wav", 3, 1)],
            environments: (0..24)
                .map(|i| if i == 2 { env(1, 3) } else { env(0, 3) })
                .collect(),
        }
    }

    #[test]
    fn draw_list_repeats_by_weight_and_matches_exactly() {
        let l = lib();
        assert_eq!(l.find_sounds(0, 2, 1), vec![0, 1, 1, 1]);
        assert_eq!(l.find_sounds(0, 2, 3), vec![2]);
        assert!(l.find_sounds(0, 2, 0).is_empty());
        let mut r = Rng::new(3);
        let list = l.find_sounds(0, 2, 1);
        let picks: Vec<_> = (0..400)
            .map(|_| l.select_sound(&list, &mut r).unwrap())
            .collect();
        let b = picks.iter().filter(|&&p| p == 1).count();
        assert!((250..350).contains(&b), "{b}");
    }

    #[test]
    fn environment_needs_more_than_four_volumes() {
        assert_eq!(dominant_environment(&[0; 6]), DEFAULT_ENVIRONMENT);
        assert_eq!(
            dominant_environment(&[4, 0, 0, 0, 0, 0]),
            DEFAULT_ENVIRONMENT
        );
        assert_eq!(dominant_environment(&[0, 1, 0, 0, 7, 3]), 4);
        assert_eq!(dominant_environment(&[5, 0, 0, 5, 0, 0]), 0);
    }

    #[test]
    fn spawns_immediately_below_minimum_then_waits_for_the_delay() {
        let l = lib();
        let mut a = AmbientSequencer::new(1);
        // Below MinNumSounds: spawns at once and does not arm the delay.
        assert!(a.process(&l, 0.016, 0).is_some());
        assert_eq!(a.delay(), 0.0);
        // At the minimum with a zero delay: the roll succeeds and the delay is armed (1..2 s,
        // the bounds are swapped on purpose).
        assert!(a.process(&l, 0.016, 1).is_some());
        assert!((1.0..=2.0).contains(&a.delay()));
        // Waiting: nothing until the delay ran out.
        let armed = a.delay();
        assert!(a.process(&l, armed * 0.5, 1).is_none());
        assert!(a.process(&l, armed, 1).is_some());
        // Full house: the delay expires but nothing spawns; it is re-armed anyway.
        a.process(&l, 10.0, 3);
        assert!(a.delay() > 0.0);
    }

    #[test]
    fn spawn_rate_follows_the_delay_range() {
        // 1..3 sounds, delay 1..2 s, every sound lasts 1 s.
        let l = lib();
        let mut a = AmbientSequencer::new(9);
        let (mut t, mut spawns, mut ends) = (0.0f32, 0, Vec::<f32>::new());
        while t < 600.0 {
            ends.retain(|&e| e > t);
            if a.process(&l, 0.05, ends.len()).is_some() {
                spawns += 1;
                ends.push(t + 1.0);
            }
            t += 0.05;
        }
        // The minimum refills a silent gap at once (about 1 per second with 1 s sounds), the delay
        // adds one per 1-2 s while below the maximum.
        assert!((600..=900).contains(&spawns), "{spawns}");
    }

    #[test]
    fn water_beds_ramp_with_distance() {
        let w = WaterBeds::at(Some(5000.0), None);
        assert!((w.coast - 0.15).abs() < 1e-6 && w.waves == 0.0 && w.river == 0.0);
        let w = WaterBeds::at(Some(1000.0), Some(0.5));
        assert!((w.waves - 0.1).abs() < 1e-6);
        assert!((w.river - 0.3 * 0.18).abs() < 1e-6);
        assert_eq!(WaterBeds::at(None, Some(1.5)), WaterBeds::default());
    }

    #[test]
    fn hour_to_daytime() {
        assert_eq!(DayTime::from_hour(6.0), DayTime::Morning);
        assert_eq!(DayTime::from_hour(8.0), DayTime::Day);
        assert_eq!(DayTime::from_hour(12.0), DayTime::Day);
        assert_eq!(DayTime::from_hour(19.5), DayTime::Evening);
        assert_eq!(DayTime::from_hour(23.0), DayTime::Night);
        assert_eq!(DayTime::from_hour(2.0), DayTime::Night);
    }
}
