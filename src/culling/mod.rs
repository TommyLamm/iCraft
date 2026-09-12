//! Shared culling primitives used by authority (LOS / section occluders) and
//! by the desktop renderer.
//!
//! Section visibility traversal lives in the desktop binary's `culling`
//! facade (`main.rs`) — it must not compile into `icraft-server`.

pub mod connectivity;
pub mod los;

pub use connectivity::*;
pub use los::*;
