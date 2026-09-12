//! Block types, state encoding, and the static property table.

mod types;
mod state;
mod table;

pub use state::*;
pub use table::{BlockDef, BLOCK_TABLE, BLOCK_TYPE_COUNT};
pub use types::*;

#[cfg(test)]
mod tests;
