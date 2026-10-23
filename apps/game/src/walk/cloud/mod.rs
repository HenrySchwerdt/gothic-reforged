//! Opt-in integrations of standalone crates into `--walk` / `--play` (issue #39). Nothing here
//! runs unless asked for; without the flags below the game behaves exactly as before.
//!
//! | Flag | Environment | Crate | Module |
//! | --- | --- | --- | --- |
//! | `--mods DIR` | `GR_MODS=DIR` (`1`: `./mods`) | `gothic-mods`, `gothic-mods-script` | [`mods`], [`scripts`] |
//! | `--video FILE` | `GR_VIDEO=FILE` or `GR_VIDEO=1` (the intro) | `gothic-video` | [`video`] |
//! | `--static-render [cluster\|gpu]` | `GR_STATIC_RENDER=1\|cluster\|gpu` | `gothic-static-render` | [`statics`] |
//!
//! Flags win over the environment. See `docs/MODDING.md`, `docs/VIDEO.md` and
//! `docs/STATIC_RENDER.md` ("In the game") for what is wired and what is not verified.
pub(crate) mod mods;
pub(crate) mod scripts;
pub(crate) mod statics;
pub(crate) mod video;

use bevy::prelude::*;
use std::path::{Path, PathBuf};

/// Which video to play on start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VideoRequest {
    /// The original intro (`G3_Intro.bik`), looked up in the data directory.
    Intro,
    File(PathBuf),
}

/// How static props are culled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StaticMode {
    /// Cluster BVH culling against the view and the shadow cascades.
    pub cluster: bool,
    /// `NoCpuCulling` on the camera: per-instance frustum culling on the GPU.
    pub gpu: bool,
}

/// The opt-ins of this module, resolved from the command line and the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Options {
    pub mods: Option<PathBuf>,
    pub video: Option<VideoRequest>,
    pub static_render: Option<StaticMode>,
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .filter(|s| !s.starts_with("--"))
        .cloned()
}

fn off(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "off" | "false" | "no"
    )
}

fn on(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "on" | "true" | "yes"
    )
}

fn static_mode(v: Option<&str>) -> Option<StaticMode> {
    match v.map(|s| s.trim().to_ascii_lowercase()) {
        None => Some(StaticMode {
            cluster: true,
            gpu: true,
        }),
        Some(s) if off(&s) => None,
        Some(s) if s == "cluster" => Some(StaticMode {
            cluster: true,
            gpu: false,
        }),
        Some(s) if s == "gpu" => Some(StaticMode {
            cluster: false,
            gpu: true,
        }),
        Some(_) => Some(StaticMode {
            cluster: true,
            gpu: true,
        }),
    }
}

impl Options {
    /// Resolve from `args` and an environment lookup (`std::env::var` in the game).
    pub fn parse(args: &[String], env: impl Fn(&str) -> Option<String>) -> Self {
        let mods = match flag_value(args, "--mods") {
            Some(dir) => Some(PathBuf::from(dir)),
            None => env("GR_MODS").filter(|v| !off(v)).map(|v| {
                if on(&v) {
                    PathBuf::from("mods")
                } else {
                    PathBuf::from(v)
                }
            }),
        };
        let video = match flag_value(args, "--video") {
            Some(f) => Some(VideoRequest::File(f.into())),
            None => env("GR_VIDEO").filter(|v| !off(v)).map(|v| {
                if on(&v) || v.eq_ignore_ascii_case("intro") {
                    VideoRequest::Intro
                } else {
                    VideoRequest::File(v.into())
                }
            }),
        };
        let static_render = if args.iter().any(|a| a == "--static-render") {
            static_mode(flag_value(args, "--static-render").as_deref())
        } else {
            env("GR_STATIC_RENDER").and_then(|v| static_mode(Some(&v)))
        };
        Self {
            mods,
            video,
            static_render,
        }
    }

    pub fn from_process(args: &[String]) -> Self {
        Self::parse(args, |k| std::env::var(k).ok())
    }
}

/// Called from `walk::run` once the walk app (and, with `--play`, the play mode) is assembled.
pub(crate) fn install(app: &mut App, args: &[String], data_dir: &Path, play: bool) {
    let opts = Options::from_process(args);
    if let Some(dir) = &opts.mods
        && play
    {
        scripts::install(app, dir);
    } else if opts.mods.is_some() {
        eprintln!("mods: data and script mods need --play; ignored in --walk");
    }
    if let Some(v) = &opts.video {
        video::install(app, v, data_dir);
    }
    if let Some(mode) = opts.static_render {
        statics::install(app, mode);
    }
}

/// Synthetic test data shared by the tests of this module (no game data).
#[cfg(test)]
pub(crate) mod fixtures {
    use gothic_formats::stringtable::StringTable;

    /// A two-language string table (English, German) in the original's UTF-16 format.
    pub(crate) fn strings() -> StringTable {
        let text = "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\nT_HELLO=Hello.;;Hallo.;\r\n[LocAdmin_Revisions]\r\n";
        let mut b = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        StringTable::parse(&b).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            vars.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn cloud_options_default_off() {
        let o = Options::parse(&args(&["game", "--play"]), env(&[]));
        assert_eq!(o, Options::default());
        let o = Options::parse(
            &args(&["game"]),
            env(&[
                ("GR_MODS", "0"),
                ("GR_VIDEO", "off"),
                ("GR_STATIC_RENDER", ""),
            ]),
        );
        assert_eq!(o, Options::default());
    }

    #[test]
    fn cloud_options_flags() {
        let o = Options::parse(
            &args(&[
                "game",
                "--play",
                "--mods",
                "my/mods",
                "--video",
                "a.bik",
                "--static-render",
                "--smoke",
            ]),
            env(&[("GR_MODS", "other"), ("GR_STATIC_RENDER", "0")]),
        );
        assert_eq!(o.mods, Some(PathBuf::from("my/mods")));
        assert_eq!(o.video, Some(VideoRequest::File("a.bik".into())));
        assert_eq!(
            o.static_render,
            Some(StaticMode {
                cluster: true,
                gpu: true
            })
        );
        let o = Options::parse(&args(&["game", "--static-render", "cluster"]), env(&[]));
        assert_eq!(
            o.static_render,
            Some(StaticMode {
                cluster: true,
                gpu: false
            })
        );
        // A flag without a value does not swallow the next flag.
        let o = Options::parse(&args(&["game", "--mods", "--play"]), env(&[]));
        assert_eq!(o.mods, None);
    }

    #[test]
    fn cloud_options_env() {
        let o = Options::parse(
            &args(&["game"]),
            env(&[
                ("GR_MODS", "1"),
                ("GR_VIDEO", "1"),
                ("GR_STATIC_RENDER", "gpu"),
            ]),
        );
        assert_eq!(o.mods, Some(PathBuf::from("mods")));
        assert_eq!(o.video, Some(VideoRequest::Intro));
        assert_eq!(
            o.static_render,
            Some(StaticMode {
                cluster: false,
                gpu: true
            })
        );
        let o = Options::parse(
            &args(&["game"]),
            env(&[("GR_MODS", "/x/mods"), ("GR_VIDEO", "/v/intro.mkv")]),
        );
        assert_eq!(o.mods, Some(PathBuf::from("/x/mods")));
        assert_eq!(o.video, Some(VideoRequest::File("/v/intro.mkv".into())));
    }
}
