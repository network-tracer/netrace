//! Deterministic discrete-event simulation primitives.
//!
//! Everything in netrace runs on a virtual clock. Protocols never read wall-clock
//! time; they schedule events on a [`Scheduler`] and receive them back in a
//! strictly defined order. Given the same inputs and seed, a simulation always
//! replays identically.

mod rng;
mod scheduler;
mod time;

pub use rng::Rng;
pub use scheduler::{EventId, ScheduleError, Scheduler};
pub use time::SimTime;
