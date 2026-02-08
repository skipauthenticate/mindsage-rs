//! Resolvers — tier-aware query resolution strategies.
//!
//! Each resolver implements a different search strategy. The tier system
//! selects which resolvers are available based on device capabilities.

pub mod hybrid;
pub mod types;

pub use hybrid::HybridResolver;
pub use types::*;
