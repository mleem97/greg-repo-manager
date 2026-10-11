//! Forge backend abstraction over the gh / fj / tea CLIs.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use tracing::debug;

use crate::settings::RepoSettings;

/// Which forge a backend talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    GitHub,
    Forgejo,
    Gitea,
}

impl BackendKind {
    pub fn cli(&self) -> &'static str {
        match self {
            BackendKind::GitHub => "gh",
            BackendKind::Forgejo => "fj",
            BackendKind::Gitea => "tea",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "gh" | "github" => Some(BackendKind::GitHub),
            "fj" | "forgejo" => Some(BackendKind::Forgejo),
            "tea" | "gitea" => Some(BackendKind::Gitea),
            _ => None,
        }
    }
}

/// Capability flags: what the backend CLI can actually do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub list_repos: bool,
    pub read_settings: bool,
    pub write_general: bool,
    pub branch_protection: bool,
    pub manage_branches: bool,
    pub manage_labels: bool,
    pub manage_collaborators: bool,
    pub manage_webhooks: bool,
    pub manage_actions: bool,
    pub manage_releases: bool,
}

impl Capabilities {
    pub fn full() -> Self {
        Self {
            list_repos: true, read_settings: true, write_general: true,
            branch_protection: true, manage_branches: true, manage_labels: true,
            manage_collaborators: true, manage_webhooks: true, manage_actions: true,
            manage_releases: true,
        }
    }
}

/// Minimal repo identity + live state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub default_branch: String,
}

/// One applied change record (also used for dry-run diffs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub repo: String,
    pub area: String,
    pub detail: String,
    pub applied: bool,
}

/// Backend trait: every operation shells out to the respective CLI.
pub trait Forge: Send + Sync {
    fn kind(&self) -> BackendKind;
    fn cli_path(&self) -> String;
    fn capabilities(&self) -> Capabilities;
    fn check_auth(&self) -> Result<String>;
    fn list_repos(&self, owner: &str, limit: usize) -> Result<Vec<RepoInfo>>;
    fn read_settings(&self, owner: &str, repo: &str) -> Result<RepoSettings>;
    fn apply(&self, owner: &str, repo: &str, want: &RepoSettings, dry_run: bool) -> Result<Vec<Change>>;
}

pub fn run_cli(cli: &str, args: &[&str]) -> Result<String> {
    run_cli_stdin(cli, args, None)
}

pub fn run_cli_stdin(cli: &str, args: &[&str], input: Option<&str>) -> Result<String> {
    use std::io::Write;
    debug!("run: {} {}", cli, args.join(" "));
    let mut cmd = Command::new(cli);
    cmd.args(args);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    if input.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }
    let mut child = cmd.spawn().with_context(|| format!("spawn {cli}"))?;
    if let Some(text) = input {
        child.stdin.take().expect("piped stdin")
            .write_all(text.as_bytes()).context("write stdin")?;
    }
    let out = child.wait_with_output().context("wait")?;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("{cli} failed: {}", e.lines().next().unwrap_or("unknown error")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
