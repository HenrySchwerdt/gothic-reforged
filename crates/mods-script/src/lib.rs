//! Sandboxed script mods (Rhai): scripts subscribe to core events and issue commands through
//! a capability-limited API. No file, network, time, module or `eval` access; per-call and
//! per-tick operation budgets; a seeded RNG per mod; hot reload. See `docs/MODDING.md`.
//!
//! ```
//! use gothic_mods_script::{Capability, ScriptConfig, ScriptHost, ScriptManifest, StubCore};
//! let mut host = ScriptHost::new(ScriptConfig::default());
//! host.add_source(
//!     ScriptManifest::new("hello", &[Capability::Ui]),
//!     r#"fn on_quest_changed(e) { notice(`${e.quest}: ${e.status}`); }"#,
//! )?;
//! let mut core = StubCore::new();
//! core.set_quest("Q_Letter", "Success");
//! let commands = host.handle(&core.tick(&[]));
//! core.tick(&commands);
//! assert_eq!(core.notices, ["Q_Letter: Success"]);
//! # anyhow::Ok(())
//! ```
pub mod host;
pub mod manifest;
pub mod rng;
#[cfg(feature = "runtime")]
pub mod runtime;
pub mod stub;
pub mod vocab;

pub use host::{ErrorKind, LogLine, ScriptConfig, ScriptError, ScriptHost};
pub use manifest::{Capability, ScriptManifest};
pub use rhai;
pub use rng::Rng;
pub use stub::StubCore;
pub use vocab::{ActorInfo, Issued, Line, ScriptCommand, ScriptEvent, Vec3, WorldView};
