//! Cache management for downloaded spores and domain metadata.
//!
//! Cache location: `$CMN_HOME/hypha/cache/`
//!
//! Cache structure:
//! ```text
//! $CMN_HOME/hypha/cache/
//! └── {domain}/
//!     ├── mycelium/
//!     │   ├── cmn.json             # cached cmn.json entry
//!     │   ├── domain_state.json    # anti-rollback serial/digest/key pin
//!     │   ├── mycelium.json       # full mycelium manifest
//!     │   └── status.json         # cache status for all items
//!     │
//!     ├── repos/                  # Bare git repositories for spawn/pull
//!     │   └── {root_commit}/      # Repository identified by first commit SHA
//!     │
//!     └── spore/
//!         └── {hash}/
//!             ├── spore.json
//!             └── content/
//! ```

mod commands;
mod dir;
mod domain;
mod fs;
mod types;

pub use commands::{handle_clean, handle_list, handle_path};
pub use dir::CacheDir;
pub use domain::{DomainCache, DOMAIN_STATE_JUMP_THRESHOLD};
pub use types::{
    CacheStatus, CachedSpore, DomainStatePin, FetchStatus, KeyTrustEntry, TasteVerdictCache,
};

use crate::config::HyphaConfig;
use fs::{dir_size, locked_update_file, locked_write_file, read_spore_metadata};

#[cfg(test)]
mod tests;
