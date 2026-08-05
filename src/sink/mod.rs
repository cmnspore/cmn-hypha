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

struct AfDataSinkMessage {
    event: agent_first_data::Event,
    reply: std::sync::mpsc::SyncSender<()>,
}

fn afdata_sink_sender() -> &'static std::sync::mpsc::Sender<AfDataSinkMessage> {
    static SENDER: std::sync::OnceLock<std::sync::mpsc::Sender<AfDataSinkMessage>> =
        std::sync::OnceLock::new();
    SENDER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<AfDataSinkMessage>();
        std::thread::spawn(move || {
            let mut emitter = agent_first_data::CliEmitter::stream(
                std::io::stdout(),
                agent_first_data::OutputFormat::Json,
            )
            .with_strict_protocol();
            while let Ok(message) = receiver.recv() {
                let _ = emitter.emit(message.event);
                let _ = message.reply.send(());
            }
        });
        sender
    })
}

fn emit(event: agent_first_data::Event) {
    let (reply, result) = std::sync::mpsc::sync_channel(1);
    if afdata_sink_sender()
        .send(AfDataSinkMessage { event, reply })
        .is_ok()
    {
        let _ = result.recv();
    }
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
                        "message": crate::api::redact_urls_in_text(&message),
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
                emit(
                    agent_first_data::json_log(serde_json::json!({
                        "level": "info",
                        "message": crate::api::redact_urls_in_text(&message),
                    }))
                    .build(),
                );
            }
            HyphaEvent::Warn { message } => {
                emit(
                    agent_first_data::json_log(serde_json::json!({
                        "level": "warn",
                        "event": "warn",
                        "message": crate::api::redact_urls_in_text(&message),
                    }))
                    .build(),
                );
            }
        }
    }
}
