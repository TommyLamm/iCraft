//! Shared culling primitives used by authority (LOS / section occluders) and
//! by the desktop renderer.
//!
//! Section visibility traversal and `EntityLosManager` (worker thread) live in
//! the desktop binary's `culling` facade (`main.rs`) — they must not compile
//! into `icraft-server`.

pub mod connectivity;
pub mod los;

pub use connectivity::*;
pub use los::*;
