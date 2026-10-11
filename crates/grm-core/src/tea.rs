//! Gitea backend (`tea` CLI).
//!
//! `tea` supports repo listing/info, labels, issues/pulls/releases and
//! collaborators, but no branch protection. Areas outside its command
//! surface report `unsupported` (see [`Capabilities`]).

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::backend::{run_cli, BackendKind, Capabilities, Change, Forge, RepoInfo};
use crate::settings::*;

pub struct TeaBackend {
    pub cli: String,
    pub login: Option<String>,
}

impl TeaBackend {
    pub fn new(login: Option<String>) -> Result<Self> {
        let cli = which::which("tea").map_err(|_| anyhow!("tea CLI not found in PATH (https://gitea.com/gitea/tea)"))?;
        Ok(Self { cli: cli.to_string_lossy().into_owned(), login })
    }
    fn lv(&self, extra: &[&str]) -> Vec<String> {
        let mut a = vec![];
        if let Some(l) = &self.login {
            a.push("--login".to_string()); a.push(l.clone());
        }
        a.extend(extra.iter().map(|s| s.to_string()));
        a
    }
    fn run(&self, args: &[&str]) -> Result<String> {
        let owned = self.lv(&[]);
        let mut refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        refs.extend_from_slice(args);
        run_cli(&self.cli, &refs)
    }
}

impl Forge for TeaBackend {
    fn kind(&self) -> BackendKind { BackendKind::Gitea }
    fn cli_path(&self) -> String { self.cli.clone() }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            list_repos: true, read_settings: true, write_general: false,
            branch_protection: false, manage_branches: true, manage_labels: true,
            manage_collaborators: true, manage_webhooks: false, manage_actions: false,
            manage_releases: true,
        }
    }
    fn check_auth(&self) -> Result<String> {
        Ok(self.run(&["whoami"])?.lines().next().unwrap_or("?").to_string())
    }
    fn list_repos(&self, owner: &str, limit: usize) -> Result<Vec<RepoInfo>> {
        let out = self.run(&["repos", "list", "--owner", owner, "--limit", &limit.to_string(),
                             "--fields", "full_name,private,default_branch", "--output", "json"])?;
        let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
        Ok(v.as_array().cloned().unwrap_or_default().iter().map(|r| RepoInfo {
            owner: owner.to_string(),
            name: r["name"].as_str().unwrap_or("").to_string(),
            full_name: r["full_name"].as_str().unwrap_or("").to_string(),
            private: r["private"].as_bool().unwrap_or(false),
            default_branch: r["default_branch"].as_str().unwrap_or("main").to_string(),
        }).collect())
    }
    fn read_settings(&self, owner: &str, repo: &str) -> Result<RepoSettings> {
        let out = self.run(&["repos", "info", "--repo", &format!("{owner}/{repo}"),
                             "--fields", "description,website,private,default_branch,archived",
                             "--output", "json"])?;
        let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
        let r = v.as_array().and_then(|a| a.first().cloned()).unwrap_or(v);
        Ok(RepoSettings {
            general: Some(GeneralSettings {
                description: r["description"].as_str().map(|s| s.to_string()),
                homepage: r["website"].as_str().map(|s| s.to_string()),
                visibility: Some(if r["private"].as_bool().unwrap_or(false) { "private" } else { "public" }.to_string()),
                default_branch: r["default_branch"].as_str().map(|s| s.to_string()),
                archived: r["archived"].as_bool(),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn apply(&self, owner: &str, repo: &str, want: &RepoSettings, dry_run: bool) -> Result<Vec<Change>> {
        let full = format!("{owner}/{repo}");
        let mut out = vec![];
        if let Some(list) = &want.ensure_branches {
            for b in list {
                // tea has no branch create-from-ref; report honestly.
                out.push(Change { repo: full.clone(), area: "branch".into(),
                    detail: format!("SKIP create {} (tea: no create-from-ref command)", b.name),
                    applied: false });
            }
        }
        if let Some(list) = &want.delete_branches {
            for b in list {
                if !dry_run {
                    self.run(&["branches", "delete", "--repo", &full, "--branch", b])?;
                }
                out.push(Change { repo: full.clone(), area: "branch".into(),
                    detail: format!("delete {b}"), applied: !dry_run });
            }
        }
        if let Some(list) = &want.labels {
            for l in list {
                if !dry_run {
                    // create-or-update via delete+create (tea labels update is patchy across versions)
                    let _ = self.run(&["labels", "delete", "--repo", &full, l.name.as_str()]);
                    let mut args = vec!["labels", "create", "--repo", &full, "--name", &l.name,
                                        "--color", &l.color];
                    let desc;
                    if let Some(d) = &l.description { desc = d.clone(); args.push("--description"); args.push(&desc); }
                    self.run(&args)?;
                }
                out.push(Change { repo: full.clone(), area: "label".into(),
                    detail: format!("upsert {}", l.name), applied: !dry_run });
            }
        }
        let mut unsupported = vec![];
        if want.general.is_some() { unsupported.push("general"); }
        if want.ensure_branches.as_ref().map(|v| v.iter().any(|b| b.protection.is_some())).unwrap_or(false) {
            unsupported.push("branch-protection");
        }
        if want.collaborators.as_ref().map(|v| !v.is_empty()).unwrap_or(false) { unsupported.push("collaborators"); }
        if want.webhooks.as_ref().map(|v| !v.is_empty()).unwrap_or(false) { unsupported.push("webhooks"); }
        if want.actions.as_ref().and_then(|a| a.enabled).is_some() { unsupported.push("actions"); }
        if !unsupported.is_empty() {
            return Err(anyhow::anyhow!("unsupported by tea CLI: [{}]", unsupported.join(",")));
        }
        Ok(out)
    }
}
