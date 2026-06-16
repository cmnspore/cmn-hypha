mod commands;
mod files;
mod synapse_nodes;
mod types;

pub use commands::{handle_list, handle_set};
pub use files::{config_path, hypha_dir};
pub use synapse_nodes::{
    domain_from_url, list_synapse_domains, load_synapse_node, remove_synapse_node, resolve_synapse,
    save_synapse_node, synapse_node_dir, validate_synapse_url, ResolvedSynapse, SynapseNode,
};
pub use types::{
    CacheConfig, Defaults, HyphaConfig, KeyTrustRefreshMode, SynapseWitnessMode, TasteDefaults,
};

#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests;
