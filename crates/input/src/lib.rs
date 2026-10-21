//! Gothic 3 input layer: the session keys of the player, the action mapper that turns device
//! state into per-key press states, the character control state the movement reads, and the
//! player key router of the script layer (`OnPlayerGameKeyPressed` and its handlers).
//!
//! Everything here is Bevy-free and deterministic. A host feeds [`mapper::RawInput`] and the
//! application time every frame and gets [`mapper::GameKeyMessage`]s; [`control::CharacterControl`]
//! and [`player::PlayerInput`] turn those into the movement wish and the player actions.
pub mod control;
pub mod dik;
pub mod keys;
pub mod mapper;
pub mod player;

pub use control::{CharacterControl, ControlOptions, Look, MoveKeys};
pub use keys::{Binding, Device, PhysicalKey, SessionKey, custom_bindings_to_ini, physical_name};
pub use mapper::{ActionMapper, GameKeyMessage, RawInput};
pub use player::{Context, Order, PlayerInput};
