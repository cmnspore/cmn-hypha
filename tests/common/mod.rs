#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

pub(crate) struct TestEnv {
    _temp: TempDir,
    pub(crate) dir: PathBuf,
}

impl TestEnv {
    pub(crate) fn new() -> Self {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let dir = temp.path().to_path_buf();
        Self { _temp: temp, dir }
    }

    pub(crate) fn hypha(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_hypha"))
            .args(args)
            .env("CMN_HOME", &self.dir)
            .output()
            .expect("Failed to run hypha")
    }

    pub(crate) fn hypha_in_dir(&self, args: &[&str], cwd: &PathBuf) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_hypha"))
            .args(args)
            .current_dir(cwd)
            .env("CMN_HOME", &self.dir)
            .output()
            .expect("Failed to run hypha")
    }

    pub(crate) fn site_dir(&self, domain: &str) -> PathBuf {
        self.dir.join("mycelium").join(domain)
    }

    #[allow(dead_code)]
    pub(crate) fn hypha_with_env(
        &self,
        args: &[&str],
        env_vars: &[(&str, &str)],
    ) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_hypha"));
        cmd.args(args).env("CMN_HOME", &self.dir);
        for (key, value) in env_vars {
            cmd.env(key, value);
        }
        cmd.output().expect("Failed to run hypha")
    }
}

pub(crate) fn combined_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

pub(crate) fn parse_json_last_line(text: &str) -> serde_json::Value {
    let parsed = text
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok());
    assert!(parsed.is_some(), "failed to parse JSONL output: {}", text);
    parsed.expect("checked by assertion above")
}
