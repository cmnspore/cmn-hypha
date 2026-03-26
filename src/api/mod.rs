#![allow(clippy::print_stdout)]
// Agent-First Data output layer — the ONLY place println! should appear.
// All other code must use Output methods (ok/error/warn/progress/startup).

use serde::Serialize;
use std::process::ExitCode;

/// Result type that handles output formatting and exit codes.
///
/// Builds Agent-First Data-compliant JSON (code/result/trace/error), redacts secrets,
/// formats via agent_first_data, and prints protocol/log events to stdout.
pub struct Output {
    format: agent_first_data::OutputFormat,
}

#[allow(clippy::print_stdout)]
impl Output {
    pub fn new(format: agent_first_data::OutputFormat) -> Self {
        Self { format }
    }

    fn format(&self, value: &serde_json::Value) -> String {
        agent_first_data::cli_output(value, self.format)
    }

    /// {code: "ok", result: ...} → stdout
    pub fn ok<T: Serialize>(&self, result: T) -> ExitCode {
        let result_value = serde_json::to_value(&result).unwrap_or_default();
        let mut resp = agent_first_data::build_json_ok(result_value, None);
        agent_first_data::internal_redact_secrets(&mut resp);
        println!("{}", self.format(&resp));
        ExitCode::SUCCESS
    }

    /// {code: "ok", result: ..., trace: ...} → stdout
    pub fn ok_trace<T: Serialize>(&self, result: T, trace: impl Serialize) -> ExitCode {
        let result_value = serde_json::to_value(&result).unwrap_or_default();
        let trace_value = serde_json::to_value(&trace).unwrap_or_default();
        let mut resp = agent_first_data::build_json_ok(result_value, Some(trace_value));
        agent_first_data::internal_redact_secrets(&mut resp);
        println!("{}", self.format(&resp));
        ExitCode::SUCCESS
    }

    /// {code: "<error_code>", error: "msg", hint?: "...", trace: {duration_ms: 0}} → stdout
    pub fn error(&self, error_code: &str, message: &str) -> ExitCode {
        self.error_hint(error_code, message, None)
    }

    /// Like [`error`] but with an actionable hint for remediation.
    pub fn error_hint(&self, error_code: &str, message: &str, hint: Option<&str>) -> ExitCode {
        let mut fields = serde_json::Map::new();
        fields.insert(
            "error".into(),
            serde_json::Value::String(message.to_string()),
        );
        if let Some(h) = hint {
            fields.insert("hint".into(), serde_json::Value::String(h.to_string()));
        }
        let mut resp = agent_first_data::build_json(
            error_code,
            serde_json::Value::Object(fields),
            Some(serde_json::json!({"duration_ms": 0})),
        );
        agent_first_data::internal_redact_secrets(&mut resp);
        println!("{}", self.format(&resp));
        ExitCode::FAILURE
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
    /// {"code": "progress", "current": N, "total": M, "message": "...", ...}
    pub fn progress(&self, step: u32, total: u32, message: &str, data: serde_json::Value) {
        let mut fields = match data {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        fields.insert("current".into(), step.into());
        fields.insert("total".into(), total.into());
        fields.insert("message".into(), message.into());
        let mut resp =
            agent_first_data::build_json("progress", serde_json::Value::Object(fields), None);
        agent_first_data::internal_redact_secrets(&mut resp);
        println!("{}", self.format(&resp));
    }

    /// Byte-level download progress → stdout
    /// {"code": "download_progress", "downloaded_bytes": N, "total_bytes": M}
    pub fn download_progress(&self, downloaded_bytes: u64, total_bytes: Option<u64>) {
        let mut resp = agent_first_data::build_json(
            "download_progress",
            serde_json::json!({
                "downloaded_bytes": downloaded_bytes,
                "total_bytes": total_bytes,
            }),
            None,
        );
        agent_first_data::internal_redact_secrets(&mut resp);
        println!("{}", self.format(&resp));
    }

    /// Non-fatal warning → stdout
    pub fn warn(&self, code: &str, message: &str) {
        let mut resp =
            agent_first_data::build_json(code, serde_json::json!({"message": message}), None);
        agent_first_data::internal_redact_secrets(&mut resp);
        println!("{}", self.format(&resp));
    }

    /// {code: "log", event: "startup", args: ..., config: ..., env: ...} → stdout
    pub fn startup(&self, args: serde_json::Value) {
        let cfg = crate::config::HyphaConfig::load();
        let config = serde_json::to_value(&cfg).unwrap_or_default();

        let env = serde_json::json!({
            "CMN_HOME": std::env::var("CMN_HOME").ok(),
            "SYNAPSE_TOKEN_SECRET": std::env::var("SYNAPSE_TOKEN_SECRET").ok(),
        });
        let mut resp = agent_first_data::build_json(
            "log",
            serde_json::json!({
                "event": "startup",
                "hypha_version": env!("CARGO_PKG_VERSION"),
                "config": config,
                "args": args,
                "env": env
            }),
            None,
        );
        agent_first_data::internal_redact_secrets(&mut resp);
        println!("{}", self.format(&resp));
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
