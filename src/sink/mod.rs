/// A structured event emitted by hypha library functions.
///
/// Callers decide how to handle these — print them (CLI), forward to a channel
/// (GUI/agent), or ignore them entirely (NoopSink).
#[derive(Debug, Clone)]
pub enum HyphaEvent {
    /// A multi-step operation made progress.
    Progress {
        current: u32,
        total: u32,
        message: String,
    },
    /// Byte-level download progress for speed/ETA display.
    DownloadProgress {
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    /// A non-fatal informational message.
    Log { message: String },
    /// A non-fatal warning.
    Warn { message: String },
}

/// Receiver of events emitted during a hypha operation.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: HyphaEvent);
}

/// Discards all events — use when the caller only cares about the return value.
pub struct NoopSink;

impl EventSink for NoopSink {
    fn emit(&self, _: HyphaEvent) {}
}

/// Structured error returned by hypha library functions.
///
/// The `code` field is a machine-readable identifier (e.g. `"invalid_uri"`,
/// `"dns_failed"`).  The `message` is human-readable detail.
/// The optional `hint` provides actionable remediation advice.
#[derive(Debug, Clone)]
pub struct HyphaError {
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
}

impl HyphaError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            hint: None,
        }
    }

    pub fn with_hint(
        code: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            hint: Some(hint.into()),
        }
    }
}

impl std::fmt::Display for HyphaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HyphaError {}

impl From<String> for HyphaError {
    fn from(msg: String) -> Self {
        Self {
            code: "error".to_string(),
            message: msg,
            hint: None,
        }
    }
}

impl From<&str> for HyphaError {
    fn from(msg: &str) -> Self {
        Self::from(msg.to_string())
    }
}

/// Emits events as afdata JSON to stdout — used by library callers.
pub struct AfDataSink;

fn emit(event: agent_first_data::Event) {
    let stdout = std::io::stdout();
    let mut emitter =
        agent_first_data::CliEmitter::new(stdout.lock(), agent_first_data::OutputFormat::Json);
    let _ = emitter.emit(event);
}

impl EventSink for AfDataSink {
    fn emit(&self, event: HyphaEvent) {
        match event {
            HyphaEvent::Progress {
                current,
                total,
                message,
            } => {
                emit(
                    agent_first_data::json_progress(serde_json::json!({
                        "current": current,
                        "total": total,
                        "message": message,
                    }))
                    .build(),
                );
            }
            HyphaEvent::DownloadProgress {
                downloaded_bytes,
                total_bytes,
            } => {
                emit(
                    agent_first_data::json_progress(serde_json::json!({
                        "event": "download_progress",
                        "downloaded_bytes": downloaded_bytes,
                        "total_bytes": total_bytes,
                    }))
                    .build(),
                );
            }
            HyphaEvent::Log { message } => {
                emit(agent_first_data::json_log(serde_json::json!({ "message": message })).build());
            }
            HyphaEvent::Warn { message } => {
                emit(
                    agent_first_data::json_log(
                        serde_json::json!({ "event": "warn", "message": message }),
                    )
                    .build(),
                );
            }
        }
    }
}
