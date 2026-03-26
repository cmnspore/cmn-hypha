#![allow(dead_code)]
//! Unified progress step definitions for all visitor operations.
//!
//! Each operation defines its steps as a static array. Use `emit_step`
//! to emit progress events with consistent formatting.

pub struct OpSteps {
    pub name: &'static str,
    pub steps: &'static [&'static str],
}

impl OpSteps {
    pub fn total(&self) -> u32 {
        self.steps.len() as u32
    }
}

pub const SENSE: OpSteps = OpSteps {
    name: "sense",
    steps: &[
        "Resolving URI",
        "Fetching cmn.json",
        "Fetching manifest",
        "Verifying signatures",
    ],
};

pub const SPAWN: OpSteps = OpSteps {
    name: "spawn",
    steps: &[
        "Resolving URI",
        "Fetching cmn.json",
        "Checking taste",
        "Downloading",
        "Verifying",
        "Extracting",
    ],
};

pub const GROW: OpSteps = OpSteps {
    name: "grow",
    steps: &[
        "Reading spore.core.json",
        "Querying Synapse lineage",
        "Verifying new spore",
        "Checking taste verdict",
        "Applying update",
        "Complete",
    ],
};

pub const TASTE: OpSteps = OpSteps {
    name: "taste",
    steps: &[
        "Resolving URI",
        "Fetching cmn.json",
        "Downloading",
        "Verifying",
        "Recording verdict",
    ],
};

pub const BOND: OpSteps = OpSteps {
    name: "bond",
    steps: &[
        "Reading manifest",
        "Checking taste",
        "Fetching bonds",
        "Building index",
    ],
};

pub const FETCH_SPORE: OpSteps = OpSteps {
    name: "fetch_spore",
    steps: &[
        "Fetching cmn.json",
        "Fetching spore manifest",
        "Verifying spore",
        "Preparing",
        "Downloading content",
        "Verifying content hash",
    ],
};

/// Emit a progress event for the given step index (0-based).
pub fn emit_step(sink: &dyn crate::EventSink, steps: &OpSteps, index: usize) {
    if index < steps.steps.len() {
        sink.emit(crate::HyphaEvent::Progress {
            current: (index + 1) as u32,
            total: steps.total(),
            message: steps.steps[index].to_string(),
        });
    }
}
