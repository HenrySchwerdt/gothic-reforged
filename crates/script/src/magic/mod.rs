//! Magic: the casting pipeline ([`cast`]), the spell effects of B20 ([`effects`]), the area and
//! status states of B21 ([`area`]) and the host interface ([`api`]).

pub mod api;
pub mod area;
pub mod cast;
pub mod effects;

#[cfg(test)]
mod tests;
