//! Forgejo backend (`fj` CLI).
//!
//! Honest capability note: `fj` is minimal (view/create/delete repos,
//! actions variables/secrets/dispatch). It cannot list repos, edit repo
//! settings, or manage branches/labels/protection — those report
//! `unsupported` and the UI disables them.

use anyhow::{anyhow, Result};

use crate::backend::{run_cli, BackendKind, Capabilities, Change, Forge, RepoInfo};
use crate::settings::RepoSettings;

pub struct FjBackend {
    pub cli: String,
    pub host: Option<String>,
}

impl FjBackend {
    pub fn new(host: Option<String>) -> Result<Self> {
        let cli = which::which("fj").map_err(|_| anyhow!("fj CLI not found in PATH"))?;
        Ok(Self { cli: cli.to_string_lossy().into_owned(), host })
    }
}

impl Forge for FjBackend {
    fn kind(&self) -> BackendKind { BackendKind::Forgejo }
    fn cli_path(&self) -> String { self.cli.clone() }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            list_repos: false, read_settings: true, write_general: false,
            branch_protection: false, manage_branches: false, manage_labels: false,
            manage_collaborators: false, manage_webhooks: false, manage_actions: false,
            manage_releases: false,
        }
    }
    fn check_auth(&self) -> Result<String> {
        let out = match &self.host {
            Some(h) => run_cli(&self.cli, &["whoami", "-H", h])?,
            None => run_cli(&self.cli, &["whoami"])?,
        };
        Ok(out.lines().next().unwrap_or("?").to_string())
    }
    fn list_repos(&self, _owner: &str, _limit: usize) -> Result<Vec<RepoInfo>> {
        Err(anyhow!("unsupported: `fj` cannot list repositories (no list subcommand)"))
    }
    fn read_settings(&self, owner: &str, repo: &str) -> Result<RepoSettings> {
        // `fj repo view` prints human-readable info; we capture only identity.
        let _ = run_cli(&self.cli, &["repo", "view", &format!("{owner}/{repo}").as_str()])?;
        Ok(RepoSettings {
            general: Some(crate::settings::GeneralSettings {
                default_branch: None, ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn apply(&self, _owner: &str, _repo: &str, want: &RepoSettings, _dry_run: bool) -> Result<Vec<Change>> {
        let mut needed = vec![];
        if want.general.as_ref().map(|g| {
            g.description.is_some() || g.homepage.is_some() || g.visibility.is_some()
                || g.default_branch.is_some() || g.topics.is_some() || g.has_issues.is_some()
                || g.has_wiki.is_some() || g.has_projects.is_some() || g.archived.is_some()
        }).unwrap_or(false) { needed.push("general"); }
        if want.ensure_branches.as_ref().map(|v| !v.is_empty()).unwrap_or(false) { needed.push("branches"); }
        if want.labels.as_ref().map(|v| !v.is_empty()).unwrap_or(false) { needed.push("labels"); }
        if want.webhooks.as_ref().map(|v| !v.is_empty()).unwrap_or(false) { needed.push("webhooks"); }
        if want.actions.as_ref().and_then(|a| a.enabled).is_some() { needed.push("actions"); }
        if needed.is_empty() { return Ok(vec![]); }
        Err(anyhow!("unsupported: `fj` cannot apply [{}] (CLI has no such commands)", needed.join(",")))
    }
}
