// Agent-First Data output layer. All CLI code emits through Output methods.

use serde::Serialize;
use std::process::ExitCode;

/// Result type that handles output formatting and exit codes.
///
/// Builds and emits Agent-First Data protocol events through [`agent_first_data::CliEmitter`].
pub struct Output {
    format: agent_first_data::OutputFormat,
}

fn built_event(
    result: Result<agent_first_data::Event, agent_first_data::BuildError>,
) -> agent_first_data::Event {
    result.unwrap_or_else(|err| agent_first_data::build_cli_error(&err.to_string(), None))
}

impl Output {
    pub fn new(format: agent_first_data::OutputFormat) -> Self {
        Self { format }
    }

    /// Best-effort emit for mid-stream events (progress/log/warn): a failed
    /// write (including a hung-up reader) is intentionally ignored so
    /// diagnostics never abort the command.
    fn emit(&self, event: agent_first_data::Event) {
        let stdout = std::io::stdout();
        let mut emitter = agent_first_data::CliEmitter::new(stdout.lock(), self.format);
        let _ = emitter.emit(event);
    }

    /// Emit a terminal event (`result`/`error`) and resolve the process exit
    /// code the broken-pipe-safe way, mirroring agent-first-data's one-shot CLI
    /// contract ([`agent_first_data::CliEmitter::finish`]): `success_code` on a
    /// clean write, `0` if the reader hung up (broken pipe), `4` on any other
    /// write/validation failure. Replaces the old "emit-then-return-a-fixed-code"
    /// dance, which reported success even when the terminal event failed to write.
    fn finish(&self, event: agent_first_data::Event, success_code: u8) -> ExitCode {
        let stdout = std::io::stdout();
        let mut emitter = agent_first_data::CliEmitter::new(stdout.lock(), self.format);
        ExitCode::from(emitter.finish(event, success_code))
    }

    /// `{kind: "result", result: ..., trace: {}}` → stdout.
    pub fn ok<T: Serialize>(&self, result: T) -> ExitCode {
        let result_value = serde_json::to_value(&result).unwrap_or_default();
        self.finish(agent_first_data::json_result(result_value).build(), 0)
    }

    /// `{kind: "result", result: ..., trace: ...}` → stdout.
    pub fn ok_trace<T: Serialize>(&self, result: T, trace: impl Serialize) -> ExitCode {
        let result_value = serde_json::to_value(&result).unwrap_or_default();
        let trace_value = serde_json::to_value(&trace).unwrap_or_default();
        self.finish(
            agent_first_data::json_result(result_value)
                .trace(trace_value)
                .build(),
            0,
        )
    }

    /// `{kind: "result", result: ...}` → stdout, with a caller-chosen exit code.
    ///
    /// For machine-checkable "is there drift" style results (e.g. `hatch bond
    /// sync --check`) where the outcome is a normal, well-formed result (not an
    /// error) but CI still needs a distinct non-zero exit to gate on.
    pub fn ok_with_code<T: Serialize>(&self, result: T, code: u8) -> ExitCode {
        let result_value = serde_json::to_value(&result).unwrap_or_default();
        self.finish(agent_first_data::json_result(result_value).build(), code)
    }

    /// `{kind: "error", error: {code, message, hint, retryable}, trace}` → stdout.
    pub fn error(&self, error_code: &str, message: &str) -> ExitCode {
        self.error_hint(error_code, message, None)
    }

    /// Like [`error`] but with an actionable hint for remediation.
    pub fn error_hint(&self, error_code: &str, message: &str, hint: Option<&str>) -> ExitCode {
        self.finish(
            built_event(
                agent_first_data::json_error(error_code, message)
                    .hint(actionable_hint(error_code, hint))
                    .trace(serde_json::json!({"duration_ms": 0}))
                    .build(),
            ),
            1,
        )
    }

    /// Output error from anyhow::Error
    pub fn error_from(&self, error_code: &str, err: &anyhow::Error) -> ExitCode {
        self.error(error_code, &err.to_string())
    }

    /// Output error from [`crate::HyphaError`] (includes hint when present).
    pub fn error_hypha(&self, err: &crate::HyphaError) -> ExitCode {
        self.error_hint(&err.code, &err.message, err.hint.as_deref())
    }

    /// Agent-First Data progress step → stdout
    /// `{kind: "progress", progress: {current, total, message, ...}, trace}`.
    pub fn progress(&self, step: u32, total: u32, message: &str, data: serde_json::Value) {
        let mut fields = match data {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        fields.insert("current".into(), step.into());
        fields.insert("total".into(), total.into());
        fields.insert("message".into(), message.into());
        self.emit(agent_first_data::json_progress(serde_json::Value::Object(fields)).build());
    }

    /// Byte-level protocol-v1 download progress → stdout.
    /// `{kind: "progress", progress: {event, downloaded_bytes, total_bytes}}`.
    pub fn download_progress(&self, downloaded_bytes: u64, total_bytes: Option<u64>) {
        self.emit(
            agent_first_data::json_progress(serde_json::json!({
                "event": "download_progress",
                "downloaded_bytes": downloaded_bytes,
                "total_bytes": total_bytes,
            }))
            .build(),
        );
    }

    /// Non-fatal warning → stdout
    pub fn warn(&self, code: &str, message: &str) {
        self.emit(
            agent_first_data::json_log(serde_json::json!({
                "event": code,
                "message": message,
            }))
            .build(),
        );
    }

    /// Emit a protocol-v1 startup log event to stdout.
    pub fn startup(&self, args: serde_json::Value) {
        let (config, config_error) = match crate::config::HyphaConfig::load() {
            Ok(cfg) => (serde_json::to_value(&cfg).unwrap_or_default(), None),
            Err(err) => (
                serde_json::Value::Null,
                Some(serde_json::json!({
                    "code": err.code,
                    "message": err.message,
                    "hint": err.hint,
                })),
            ),
        };

        let env = serde_json::json!({
            "CMN_HOME": std::env::var("CMN_HOME").ok(),
            "SYNAPSE_TOKEN_SECRET": std::env::var("SYNAPSE_TOKEN_SECRET").ok(),
        });
        self.emit(
            agent_first_data::json_log(serde_json::json!({
                "category": "startup",
                "event": "startup",
                "hypha_version": env!("CARGO_PKG_VERSION"),
                "config": config,
                "config_error": config_error,
                "args": args,
                "env": env
            }))
            .build(),
        );
    }
}

fn actionable_hint<'a>(error_code: &str, hint: Option<&'a str>) -> &'a str {
    if let Some(hint) = hint.filter(|h| !h.trim().is_empty()) {
        return hint;
    }
    match error_code {
        "invalid_args" | "invalid_value" | "unknown_key" | "missing_domain" => {
            "run hypha --help or the relevant subcommand --help, then retry with valid inputs"
        }
        "invalid_uri" | "uri_error" | "cmn_invalid" => {
            "use a CMN URI in the form cmn://domain or cmn://domain/b3.hash"
        }
        "synapse_error" | "network_error" | "NETWORK_ERR" => {
            "check the synapse URL, network connectivity, and any configured auth token"
        }
        "not_found" | "missing_spore" | "not_cached" | "spore_not_found" => {
            "verify the domain/hash and run hypha sense or hypha cache list before retrying"
        }
        "spore_security_rejected" => {
            "received content targets protected control paths; inspect the spore and only relax cache.spore_reject_path_components if you accept that risk"
        }
        "skill_error" => "run hypha skill --help and retry with the suggested options",
        _ => "read the error field, check hypha --help for the expected input, and retry",
    }
}

/// Bridges [`crate::EventSink`] to an existing [`Output`].
///
/// Used in `handle_*` CLI wrappers so inner lib functions (which now take
/// `&dyn EventSink`) can still emit warnings through the normal CLI output.
pub struct OutSink<'a>(pub &'a Output);

impl crate::EventSink for OutSink<'_> {
    fn emit(&self, event: crate::HyphaEvent) {
        match event {
            crate::HyphaEvent::Progress {
                current,
                total,
                message,
            } => {
                self.0
                    .progress(current, total, &message, serde_json::Value::Null);
            }
            crate::HyphaEvent::DownloadProgress {
                downloaded_bytes,
                total_bytes,
            } => {
                self.0.download_progress(downloaded_bytes, total_bytes);
            }
            crate::HyphaEvent::Log { message } => {
                self.0.warn("log", &message);
            }
            crate::HyphaEvent::Warn { message } => {
                self.0.warn("warn", &message);
            }
        }
    }
}
