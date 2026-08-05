// Agent-First Data output layer. All CLI code emits through Output methods.

use serde::Serialize;
use std::process::ExitCode;
use std::sync::{mpsc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// Result type that handles output formatting and exit codes.
///
/// A dedicated writer thread owns one [`agent_first_data::CliEmitter`] for the
/// complete command lifecycle. This keeps [`Output`] usable through the
/// thread-safe [`crate::EventSink`] trait while preserving AFDATA ordering,
/// routing, and single-terminal-event enforcement.
pub struct Output {
    sender: mpsc::Sender<OutputMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
    started: Instant,
}

enum OutputMessage {
    Emit(agent_first_data::Event),
    Finish {
        event: agent_first_data::Event,
        success_code: u8,
        reply: mpsc::SyncSender<u8>,
    },
    Shutdown,
}

fn built_event(
    result: Result<agent_first_data::Event, agent_first_data::BuildError>,
) -> agent_first_data::Event {
    result.unwrap_or_else(|err| agent_first_data::build_cli_error(&err.to_string(), None))
}

/// Scrub every scheme-prefixed URL embedded in diagnostic prose.
///
/// AFDATA intentionally does not scan arbitrary prose, so Hypha performs this
/// boundary pass before placing messages into protocol events.
pub fn redact_urls_in_text(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut rendered = String::with_capacity(text.len());
    let mut copied_until = 0;
    let mut search_from = 0;

    while let Some(relative) = text[search_from..].find("://") {
        let separator = search_from + relative;
        let mut start = separator;
        while start > 0
            && bytes[start - 1].is_ascii()
            && (bytes[start - 1].is_ascii_alphanumeric()
                || matches!(bytes[start - 1], b'+' | b'-' | b'.'))
        {
            start -= 1;
        }
        if start == separator || !bytes[start].is_ascii_alphabetic() {
            search_from = separator + 3;
            continue;
        }

        let mut end = separator + 3;
        while end < bytes.len()
            && !bytes[end].is_ascii_whitespace()
            && !matches!(
                bytes[end],
                b'"' | b'\'' | b'<' | b'>' | b'{' | b'}' | b'\\' | b'|'
            )
        {
            end += 1;
        }
        while end > separator + 3
            && matches!(
                bytes[end - 1],
                b'.' | b',' | b';' | b':' | b'!' | b')' | b']'
            )
        {
            end -= 1;
        }

        rendered.push_str(&text[copied_until..start]);
        rendered.push_str(&agent_first_data::redact_url_secrets(&text[start..end]));
        copied_until = end;
        search_from = end.max(separator + 3);
    }

    rendered.push_str(&text[copied_until..]);
    rendered
}

/// Redact credential-bearing values in embedded CMN documents without
/// rewriting their protocol-defined `url` field name.
///
/// Hypha-owned URL fields use AFDATA's `_url` suffix and are scrubbed by the
/// emitter. CMN v1 documents instead define bare `url` fields, so those values
/// need this schema-preserving adapter before they reach any serializer.
fn redact_embedded_cmn_urls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, field) in fields {
                if name == "url" {
                    if let serde_json::Value::String(raw_url) = field {
                        *raw_url = agent_first_data::redact_url_secrets(raw_url);
                    }
                }
                redact_embedded_cmn_urls(field);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_embedded_cmn_urls(item);
            }
        }
        _ => {}
    }
}

/// A Synapse selector accepts either a configured domain or a whole URL. Keep
/// the selector name honest while scrubbing it whenever the supplied value is
/// URL-shaped.
fn redact_url_selectors(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, field) in fields {
                if name == "synapse_selector" {
                    if let serde_json::Value::String(selector) = field {
                        *selector = redact_urls_in_text(selector);
                    }
                }
                redact_url_selectors(field);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_url_selectors(item);
            }
        }
        _ => {}
    }
}

fn redact_boundary_values(value: &mut serde_json::Value) {
    redact_embedded_cmn_urls(value);
    redact_url_selectors(value);
}

impl Output {
    /// Create a finite one-shot emitter using AFDATA's default split routing.
    pub fn new(format: agent_first_data::OutputFormat) -> Self {
        Self::with_output_to(format, agent_first_data::OutputTo::Split)
    }

    /// Create an emitter using the requested AFDATA destination policy.
    pub fn with_output_to(
        format: agent_first_data::OutputFormat,
        output_to: agent_first_data::OutputTo,
    ) -> Self {
        let started = Instant::now();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut emitter = agent_first_data::CliEmitter::from_output_to(output_to, format)
                .with_strict_protocol();
            while let Ok(message) = receiver.recv() {
                match message {
                    OutputMessage::Emit(event) => {
                        let _ = emitter.emit(event);
                    }
                    OutputMessage::Finish {
                        event,
                        success_code,
                        reply,
                    } => {
                        let code = emitter.finish(event, success_code);
                        let _ = reply.send(code);
                        break;
                    }
                    OutputMessage::Shutdown => break,
                }
            }
        });
        Self {
            sender,
            worker: Mutex::new(Some(worker)),
            started,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn elapsed_trace(&self) -> serde_json::Value {
        serde_json::json!({"duration_ms": self.elapsed_ms()})
    }

    fn with_elapsed_trace(&self, mut trace: serde_json::Value) -> serde_json::Value {
        if let serde_json::Value::Object(fields) = &mut trace {
            fields.insert("duration_ms".to_string(), self.elapsed_ms().into());
        }
        trace
    }

    /// Best-effort emit for mid-stream events (progress/log/warn): a failed
    /// write (including a hung-up reader) is intentionally ignored so
    /// diagnostics never abort the command.
    fn emit(&self, event: agent_first_data::Event) {
        let _ = self.sender.send(OutputMessage::Emit(event));
    }

    /// Emit a terminal event (`result`/`error`) and resolve the process exit
    /// code through [`agent_first_data::CliEmitter::finish`]: `success_code` on
    /// a clean write, `0` on broken pipe, and `4` on any other output failure.
    fn finish(&self, event: agent_first_data::Event, success_code: u8) -> ExitCode {
        let (reply, result) = mpsc::sync_channel(1);
        if self
            .sender
            .send(OutputMessage::Finish {
                event,
                success_code,
                reply,
            })
            .is_err()
        {
            return ExitCode::from(4);
        }
        ExitCode::from(result.recv().unwrap_or(4))
    }

    /// `{kind: "result", result: ..., trace: {duration_ms}}` → stdout.
    pub fn ok<T: Serialize>(&self, result: T) -> ExitCode {
        match serde_json::to_value(&result) {
            Ok(mut result_value) => {
                redact_boundary_values(&mut result_value);
                self.finish(
                    agent_first_data::json_result(result_value)
                        .trace(self.elapsed_trace())
                        .build(),
                    0,
                )
            }
            Err(err) => self.error(
                "serialize_error",
                &format!("Failed to serialize command result: {err}"),
            ),
        }
    }

    /// `{kind: "result", result: ..., trace: ...}` → stdout.
    pub fn ok_trace<T: Serialize>(&self, result: T, trace: impl Serialize) -> ExitCode {
        let mut result_value = match serde_json::to_value(&result) {
            Ok(value) => value,
            Err(err) => {
                return self.error(
                    "serialize_error",
                    &format!("Failed to serialize command result: {err}"),
                )
            }
        };
        let mut trace_value = match serde_json::to_value(&trace) {
            Ok(value) if value.is_object() => value,
            Ok(_) => {
                return self.error(
                    "serialize_error",
                    "Command trace must serialize as an object",
                )
            }
            Err(err) => {
                return self.error(
                    "serialize_error",
                    &format!("Failed to serialize command trace: {err}"),
                )
            }
        };
        redact_boundary_values(&mut result_value);
        redact_boundary_values(&mut trace_value);
        self.finish(
            agent_first_data::json_result(result_value)
                .trace(self.with_elapsed_trace(trace_value))
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
        match serde_json::to_value(&result) {
            Ok(mut result_value) => {
                redact_boundary_values(&mut result_value);
                self.finish(
                    agent_first_data::json_result(result_value)
                        .trace(self.elapsed_trace())
                        .build(),
                    code,
                )
            }
            Err(err) => self.error(
                "serialize_error",
                &format!("Failed to serialize command result: {err}"),
            ),
        }
    }

    /// `{kind: "error", error: {code, message, hint, retryable}, trace}` →
    /// the configured diagnostic destination.
    pub fn error(&self, error_code: &str, message: &str) -> ExitCode {
        self.error_hint(error_code, message, None)
    }

    /// Like [`error`] but with an actionable hint for remediation.
    pub fn error_hint(&self, error_code: &str, message: &str, hint: Option<&str>) -> ExitCode {
        let message = redact_urls_in_text(message);
        let hint = redact_urls_in_text(actionable_hint(error_code, hint));
        self.finish(
            built_event(
                agent_first_data::json_error(error_code, &message)
                    .hint(&hint)
                    .trace(self.elapsed_trace())
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

    /// Agent-First Data progress step → the configured diagnostic destination.
    /// `{kind: "progress", progress: {current, total, message, ...}, trace}`.
    pub fn progress(&self, step: u32, total: u32, message: &str, data: serde_json::Value) {
        let mut fields = match data {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        fields.insert("current".into(), step.into());
        fields.insert("total".into(), total.into());
        fields.insert("message".into(), redact_urls_in_text(message).into());
        let mut progress = serde_json::Value::Object(fields);
        redact_boundary_values(&mut progress);
        self.emit(
            agent_first_data::json_progress(progress)
                .trace(self.elapsed_trace())
                .build(),
        );
    }

    /// Byte-level protocol-v1 download progress → the configured diagnostic destination.
    /// `{kind: "progress", progress: {event, downloaded_bytes, total_bytes}}`.
    pub fn download_progress(&self, downloaded_bytes: u64, total_bytes: Option<u64>) {
        self.emit(
            agent_first_data::json_progress(serde_json::json!({
                "event": "download_progress",
                "downloaded_bytes": downloaded_bytes,
                "total_bytes": total_bytes,
            }))
            .trace(self.elapsed_trace())
            .build(),
        );
    }

    /// Non-fatal informational log → the configured diagnostic destination.
    pub fn log(&self, code: &str, message: &str) {
        self.log_data(
            "info",
            code,
            message,
            serde_json::Value::Object(Default::default()),
        );
    }

    /// Structured non-fatal log with additional tool-defined fields.
    pub fn log_data(&self, level: &str, code: &str, message: &str, data: serde_json::Value) {
        let mut fields = match data {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        fields.insert("level".into(), level.into());
        fields.insert("event".into(), code.into());
        fields.insert("message".into(), redact_urls_in_text(message).into());
        let mut log = serde_json::Value::Object(fields);
        redact_boundary_values(&mut log);
        self.emit(
            agent_first_data::json_log(log)
                .trace(self.elapsed_trace())
                .build(),
        );
    }

    /// Non-fatal warning → the configured diagnostic destination.
    pub fn warn(&self, code: &str, message: &str) {
        self.log_data(
            "warn",
            code,
            message,
            serde_json::Value::Object(Default::default()),
        );
    }

    /// Emit a protocol-v1 startup log event to the diagnostic destination.
    pub fn startup(&self, args: serde_json::Value) {
        let (config, config_error) = match crate::config::HyphaConfig::load() {
            Ok(cfg) => match serde_json::to_value(&cfg) {
                Ok(config) => (config, None),
                Err(err) => (
                    serde_json::Value::Null,
                    Some(serde_json::json!({
                        "code": "config_serialize_error",
                        "message": err.to_string(),
                    })),
                ),
            },
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
        self.log_data(
            "info",
            "startup",
            "Hypha command started",
            serde_json::json!({
                "category": "startup",
                "config": config,
                "config_error": config_error,
                "args": args,
                "env": env
            }),
        );
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.sender.send(OutputMessage::Shutdown);
        if let Ok(worker) = self.worker.get_mut() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
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
                self.0.log("log", &message);
            }
            crate::HyphaEvent::Warn { message } => {
                self.0.warn("warn", &message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{redact_boundary_values, redact_embedded_cmn_urls, redact_urls_in_text};

    #[test]
    fn diagnostic_prose_scrubs_urls_without_damaging_punctuation() {
        let secret = "diagnostic-query-secret";
        let message = format!(
            "request failed (https://user:password@example.com/api?token_secret={secret}), fallback cmn://example.com/b3.test"
        );

        let redacted = redact_urls_in_text(&message);

        assert!(!redacted.contains("password"));
        assert!(!redacted.contains(secret));
        assert!(redacted.contains("), fallback cmn://example.com/b3.test"));
    }

    #[test]
    fn embedded_cmn_url_is_scrubbed_without_renaming_the_protocol_field() {
        let secret = "embedded-cmn-secret";
        let mut document = serde_json::json!({
            "$schema": "https://cmn.dev/schemas/v1/spore.json",
            "capsule": {
                "uri": "cmn://example.com/b3.test",
                "dist": [{
                    "type": "git",
                    "url": format!(
                        "https://user:password@example.com/repo?token_secret={secret}"
                    )
                }]
            }
        });

        redact_embedded_cmn_urls(&mut document);

        let distribution = &document["capsule"]["dist"][0];
        let rendered_url = distribution["url"].as_str().unwrap();
        assert!(!rendered_url.contains("password"));
        assert!(!rendered_url.contains(secret));
        assert!(distribution.get("url").is_some());
        assert!(distribution.get("distribution_url").is_none());
        assert_eq!(document["capsule"]["uri"], "cmn://example.com/b3.test");
    }

    #[test]
    fn synapse_selector_scrubs_urls_but_preserves_domain_selectors() {
        let secret = "selector-secret";
        let mut values = serde_json::json!([
            {"synapse_selector": "synapse.example.com"},
            {
                "synapse_selector": format!(
                    "https://user:password@synapse.example.com?token_secret={secret}"
                )
            }
        ]);

        redact_boundary_values(&mut values);

        assert_eq!(values[0]["synapse_selector"], "synapse.example.com");
        let rendered_selector = values[1]["synapse_selector"].as_str().unwrap();
        assert!(!rendered_selector.contains("password"));
        assert!(!rendered_selector.contains(secret));
    }
}
