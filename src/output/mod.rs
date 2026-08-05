//! Typed result structs returned by hypha library functions.
//!
//! Each command gets its own output type so callers don't need to
//! parse `serde_json::Value`.  The CLI `handle_*` wrappers serialize
//! these into Agent-First Data JSON before printing.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ───────────────────────── sense ─────────────────────────

/// Result of [`crate::visitor::sense`].
///
/// `data` is `{"mycelium": manifest}` or `{"spore": manifest}` depending on
/// the URI content type.  `trace` carries resolution metadata (DNS, caching,
/// signature verification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenseOutput {
    pub cmn_url: String,
    /// Resolved content — a mycelium manifest or spore manifest.
    pub data: Value,
    /// Resolution metadata: DNS, cmn.json caching, signature verification.
    pub trace: Value,
}

// ───────────────────────── search ────────────────────────

/// Result of [`crate::visitor::search`] / [`crate::visitor::search_with_bond`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOutput {
    pub query: String,
    pub synapse_url: String,
    pub result_count: usize,
    pub results: Vec<SearchResult>,
}

/// One Hypha-owned search result adapted from the Synapse wire response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub cmn_url: String,
    pub domain: String,
    pub name: String,
    pub synopsis: String,
    pub license: String,
    pub intent: Vec<String>,
    pub relevance: f32,
}

// ───────────────────────── taste ─────────────────────────

/// Result of `taste` (download mode — no verdict supplied).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteDownloadOutput {
    pub cmn_url: String,
    pub cache_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_cmn_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taste: Option<TasteVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_tastes: Option<Value>,
}

/// Embedded taste verdict info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteVerdict {
    pub verdict: substrate::TasteVerdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub tasted_at_epoch_ms: u64,
}

/// Result of `taste --verdict <verdict>` (record mode).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteRecordOutput {
    pub cmn_url: String,
    pub verdict: substrate::TasteVerdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub tasted_at_epoch_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synapse_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_error: Option<String>,
}

/// Unified taste output — the library returns one of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TasteOutput {
    Download(TasteDownloadOutput),
    Record(TasteRecordOutput),
}

// ───────────────────────── spawn ─────────────────────────

/// Result of a successful `spawn`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnOutput {
    pub cmn_url: String,
    pub name: String,
    pub path: String,
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcs: Option<String>,
}

// ───────────────────────── grow ──────────────────────────

/// Result of a successful `grow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum GrowOutput {
    #[serde(rename = "updated")]
    Updated {
        cmn_url: String,
        old_hash: String,
        new_hash: String,
        method: String,
        path: String,
    },
    #[serde(rename = "up_to_date")]
    UpToDate { cmn_url: String, hash: String },
}

// ───────────────────────── bond ──────────────────────────

/// A single bonded entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BondedRef {
    pub cmn_url: String,
    pub relation: substrate::BondRelation,
    pub status: String,
}

/// Result of `bond` (default mode).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondOutput {
    pub bonded: Vec<BondedRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A single bond status entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondStatusRef {
    pub cmn_url: String,
    pub relation: substrate::BondRelation,
    /// `true`, `false`, or `"excluded"` for spawned_from/absorbed_from.
    pub bonded: Value,
}

/// Result of `bond --status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondStatusOutput {
    pub bonds: Vec<BondStatusRef>,
}

/// Result of `bond --clean`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondCleanOutput {
    pub cleaned: Vec<String>,
}

/// A bond ref with its taste status, returned when some refs are not tasted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondTasteRef {
    pub cmn_url: String,
    pub relation: substrate::BondRelation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// `"safe"`, `"rotten"`, `"toxic"`, or `"not_tasted"`.
    pub taste: String,
}

/// Returned when bond cannot proceed because some refs are not tasted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondTasteRequired {
    pub refs: Vec<BondTasteRef>,
}

/// Unified bond result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BondResult {
    Bond(BondOutput),
    Status(BondStatusOutput),
    Clean(BondCleanOutput),
    TasteRequired(BondTasteRequired),
}

// ───────────────────────── bonds (ancestors / lineage) ────

/// A node in the bond graph (ancestor or descendant).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageNode {
    pub cmn_url: String,
    pub domain: String,
    pub name: String,
    pub synopsis: String,
    pub license: String,
    pub intent: Vec<String>,
    pub relation: substrate::BondRelation,
}

/// Result of `bonds`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondsOutput {
    pub cmn_url: String,
    pub hash: String,
    pub synapse_url: String,
    pub direction: String,
    pub max_depth: u32,
    pub max_depth_reached: bool,
    pub bond_count: usize,
    pub bonds: Vec<LineageNode>,
}

// ───────────────────────── absorb ────────────────────────

/// A single absorb source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbsorbSourceInfo {
    pub cmn_url: String,
    pub hash: String,
    pub name: String,
    pub path: String,
}

/// Result of `absorb`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbsorbOutput {
    pub sources: Vec<AbsorbSourceInfo>,
    pub prompt_path: String,
}
