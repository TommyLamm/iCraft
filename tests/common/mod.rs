//! Shared real-TCP test drivers.
//!
//! These helpers intentionally stop at the NetworkClient/ServerRuntime
//! boundary.  Domain fixtures may use an authority seam to seed a world, but
//! requests and observed projections in a transport test still cross the
//! socket.

#[allow(dead_code)]
pub mod authority_harness;
#[allow(dead_code)]
pub mod tcp_harness;
