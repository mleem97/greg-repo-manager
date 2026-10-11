//! GitHub backend (`gh` CLI) — full capability set.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::backend::{run_cli, run_cli_stdin, BackendKind, Capabilities, Change, Forge, RepoInfo};
use crate::settings::*;

fn urlenc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn protection_body(p: &BranchProtection) -> String {
    let mut rev = serde_json::Map::new();
    rev.insert("dismiss_stale_reviews".into(), Value::Bool(p.dismiss_stale_reviews.unwrap_or(false)));
    rev.insert("require_code_owner_reviews".into(),
               Value::Bool(p.require_code_owner_reviews.unwrap_or(false)));
    rev.insert("required_approving_review_count".into(),
               Value::from(p.required_reviews.unwrap_or(0)));
    let checks = p.required_checks.clone().unwrap_or_default();
    let rsc = if checks.is_empty() && p.strict_up_to_date.is_none() {
        Value::Null
    } else {
        serde_json::json!({
            "strict": p.strict_up_to_date.unwrap_or(false),
            "contexts": checks,
        })
    };
    serde_json::json!({
        "required_status_checks": rsc,
        "enforce_admins": p.enforce_admins.unwrap_or(false),
        "required_pull_request_reviews": rev,
        "restrictions": null,
        "required_linear_history": p.linear_history.unwrap_or(false),
        "allow_force_pushes": p.allow_force_pushes.unwrap_or(false),
        "allow_deletions": p.allow_deletions.unwrap_or(false),
        "block_creations": false,
        "required_conversation_resolution": p.conversation_resolution.unwrap_or(false),
        "lock_branch": p.lock_branch.unwrap_or(false),
        "allow_fork_syncing": false,
    })
    .to_string()
}

pub struct GhBackend {
    pub cli: String,
}

impl GhBackend {
    pub fn new() -> Result<Self> {
        let cli = which::which("gh").map_err(|_| anyhow::anyhow!("gh CLI not found in PATH"))?;
        Ok(Self { cli: cli.to_string_lossy().into_owned() })
    }
    fn api(&self, args: &[&str]) -> Result<String> {
        run_cli(&self.cli, args)
    }
    fn api_in(&self, args: &[&str], body: &str) -> Result<String> {
        run_cli_stdin(&self.cli, args, Some(body))
    }
    fn get(&self, path: &str) -> Result<Value> {
        let out = self.api(&["api", path])?;
        serde_json::from_str(&out).context("parse gh api json")
    }
}

impl Forge for GhBackend {
    fn kind(&self) -> BackendKind { BackendKind::GitHub }
    fn cli_path(&self) -> String { self.cli.clone() }
    fn capabilities(&self) -> Capabilities { Capabilities::full() }

    fn check_auth(&self) -> Result<String> {
        let v: Value = serde_json::from_str(&self.api(&["api", "user"])?)?;
        Ok(v.get("login").and_then(|l| l.as_str()).unwrap_or("?").to_string())
    }

    fn list_repos(&self, owner: &str, limit: usize) -> Result<Vec<RepoInfo>> {
        let mut out = vec![];
        let mut page = 1;
        while out.len() < limit {
            let v: Value = serde_json::from_str(&self.api(&[
                "api", &format!("users/{owner}/repos?per_page=100&page={page}&type=all"),
            ])?)?;
            let arr = v.as_array().cloned().unwrap_or_default();
            if arr.is_empty() { break; }
            for r in arr {
                out.push(RepoInfo {
                    owner: owner.to_string(),
                    name: r["name"].as_str().unwrap_or("").to_string(),
                    full_name: r["full_name"].as_str().unwrap_or("").to_string(),
                    private: r["private"].as_bool().unwrap_or(false),
                    default_branch: r["default_branch"].as_str().unwrap_or("main").to_string(),
                });
                if out.len() >= limit { break; }
            }
            page += 1;
        }
        Ok(out)
    }

    fn read_settings(&self, owner: &str, repo: &str) -> Result<RepoSettings> {
        let base = format!("repos/{owner}/{repo}");
        let r = self.get(&base)?;
        let general = GeneralSettings {
            description: r["description"].as_str().map(|s| s.to_string()),
            homepage: r["homepage"].as_str().map(|s| s.to_string()),
            visibility: Some(if r["private"].as_bool().unwrap_or(false) { "private" } else { "public" }.to_string()),
            default_branch: r["default_branch"].as_str().map(|s| s.to_string()),
            topics: None, // separate endpoint; fetched on demand
            has_issues: r["has_issues"].as_bool(),
            has_wiki: r["has_wiki"].as_bool(),
            has_projects: r["has_projects"].as_bool(),
            has_discussions: None,
            is_template: r["is_template"].as_bool(),
            archived: r["archived"].as_bool(),
            allow_forking: r["allow_forking"].as_bool(),
            merge: Some(MergeMethods {
                allow_squash: r["allow_squash_merge"].as_bool(),
                allow_merge_commit: r["allow_merge_commit"].as_bool(),
                allow_rebase: r["allow_rebase_merge"].as_bool(),
                delete_branch_on_merge: r["delete_branch_on_merge"].as_bool(),
            }),
        };
        let mut branches: Vec<BranchSpec> = vec![];
        if let Ok(bl) = self.get(&format!("{base}/branches?per_page=100")) {
            for b in bl.as_array().cloned().unwrap_or_default() {
                let name = b["name"].as_str().unwrap_or("").to_string();
                let prot = self.get(&format!("{base}/branches/{name}/protection")).ok();
                branches.push(BranchSpec {
                    name: name.clone(),
                    from: None,
                    protection: prot.map(|p| BranchProtection {
                        required_checks: p["required_status_checks"]["checks"].as_array()
                            .map(|a| a.iter().filter_map(|c| c["context"].as_str().map(|s| s.to_string())).collect()),
                        strict_up_to_date: p["required_status_checks"]["strict"].as_bool(),
                        enforce_admins: p["enforce_admins"]["enabled"].as_bool(),
                        required_reviews: p["required_pull_request_reviews"]["required_approving_review_count"].as_u64().map(|v| v as u32),
                        dismiss_stale_reviews: p["required_pull_request_reviews"]["dismiss_stale_reviews"].as_bool(),
                        require_code_owner_reviews: p["required_pull_request_reviews"]["require_code_owner_reviews"].as_bool(),
                        linear_history: p["required_linear_history"]["enabled"].as_bool(),
                        allow_force_pushes: p["allow_force_pushes"]["enabled"].as_bool(),
                        allow_deletions: p["allow_deletions"]["enabled"].as_bool(),
                        lock_branch: p.get("lock_branch").and_then(|l| l["enabled"].as_bool()),
                        conversation_resolution: p["required_conversation_resolution"]["enabled"].as_bool(),
                    }),
                });
            }
        }
        let labels = self.get(&format!("{base}/labels?per_page=100")).ok()
            .and_then(|l| l.as_array().cloned())
            .map(|a| a.iter().map(|x| LabelSpec {
                name: x["name"].as_str().unwrap_or("").to_string(),
                color: x["color"].as_str().unwrap_or("ffffff").to_string(),
                description: x["description"].as_str().map(|s| s.to_string()),
            }).collect());
        let actions = self.get(&format!("{base}/actions/permissions")).ok()
            .map(|a| ActionsSettings { enabled: a["enabled"].as_bool() });
        Ok(RepoSettings {
            general: Some(general),
            ensure_branches: Some(branches),
            delete_branches: None,
            labels,
            delete_extra_labels: None,
            collaborators: None,
            webhooks: None,
            actions,
        })
    }

    fn apply(&self, owner: &str, repo: &str, want: &RepoSettings, dry_run: bool) -> Result<Vec<Change>> {
        let base = format!("repos/{owner}/{repo}");
        let full = format!("{owner}/{repo}");
        let mut out = vec![];
        // ---- general (single PATCH) ----
        if let Some(g) = &want.general {
            let mut body = serde_json::Map::new();
            let mut f = |k: &str, v: serde_json::Value| { body.insert(k.to_string(), v); };
            if let Some(v) = &g.description { f("description", Value::String(v.clone())); }
            if let Some(v) = &g.homepage { f("homepage", Value::String(v.clone())); }
            if let Some(v) = &g.visibility { f("private", Value::Bool(v == "private")); }
            if let Some(v) = &g.default_branch { f("default_branch", Value::String(v.clone())); }
            for (k, on) in [("has_issues", g.has_issues), ("has_wiki", g.has_wiki),
                            ("has_projects", g.has_projects), ("archived", g.archived),
                            ("allow_forking", g.allow_forking), ("is_template", g.is_template)] {
                if let Some(v) = on { f(k, Value::Bool(v)); }
            }
            if let Some(m) = &g.merge {
                for (k, on) in [("allow_squash_merge", m.allow_squash), ("allow_merge_commit", m.allow_merge_commit),
                                ("allow_rebase_merge", m.allow_rebase), ("delete_branch_on_merge", m.delete_branch_on_merge)] {
                    if let Some(v) = on { f(k, Value::Bool(v)); }
                }
            }
            if !body.is_empty() {
                let keys: Vec<String> = body.keys().cloned().collect();
                if !dry_run {
                    self.api_in(&["api", &base, "-X", "PATCH", "--input", "-"],
                                &Value::Object(body).to_string())?;
                }
                out.push(Change { repo: full.clone(), area: "general".into(),
                    detail: format!("PATCH {}", keys.join(",")), applied: !dry_run });
            }
            if let Some(topics) = &g.topics {
                if !dry_run {
                    let b = serde_json::json!({ "names": topics }).to_string();
                    self.api_in(&["api", &format!("{base}/topics"), "-X", "PUT", "--input", "-"], &b)?;
                }
                out.push(Change { repo: full.clone(), area: "topics".into(),
                    detail: format!("topics := [{}]", topics.join(", ")), applied: !dry_run });
            }
        }
        // ---- ensure branches ----
        if let Some(list) = &want.ensure_branches {
            for b in list {
                let exists = self.api(&["api", &format!("{base}/branches/{}", urlenc(&b.name))]).is_ok();
                if !exists {
                    let src = b.from.as_deref().unwrap_or("main");
                    let sha: String = self.api(&["api", &format!("{base}/git/ref/heads/{src}"),
                        "--jq", ".object.sha"]).map(|s| s.trim().trim_matches('"').to_string())
                        .unwrap_or_default();
                    if sha.len() == 40 {
                        if !dry_run {
                            let bb = serde_json::json!({"ref": format!("refs/heads/{}", b.name), "sha": sha}).to_string();
                            self.api_in(&["api", &format!("{base}/git/refs"), "--input", "-"], &bb)?;
                        }
                        out.push(Change { repo: full.clone(), area: "branch".into(),
                            detail: format!("create {} from {}", b.name, src), applied: !dry_run });
                    } else {
                        out.push(Change { repo: full.clone(), area: "branch".into(),
                            detail: format!("SKIP create {} (no sha for {})", b.name, src), applied: false });
                    }
                }
                if let Some(p) = &b.protection {
                    let body = protection_body(p);
                    if !dry_run {
                        let enc = urlenc(&b.name);
                        self.api_in(&["api", &format!("{base}/branches/{enc}/protection"),
                                      "-X", "PUT", "--input", "-"], &body)?;
                    }
                    out.push(Change { repo: full.clone(), area: "protection".into(),
                        detail: format!("protect {}", b.name), applied: !dry_run });
                }
            }
        }
        // ---- delete branches ----
        if let Some(list) = &want.delete_branches {
            for b in list {
                if !dry_run {
                    self.api(&["api", &format!("{base}/git/refs/heads/{}", urlenc(b)), "-X", "DELETE"])?;
                }
                out.push(Change { repo: full.clone(), area: "branch".into(),
                    detail: format!("delete {b}"), applied: !dry_run });
            }
        }
        // ---- labels ----
        if let Some(list) = &want.labels {
            for l in list {
                let exists = self.api(&["api", &format!("{base}/labels/{}", urlenc(&l.name))]).is_ok();
                let mut m = serde_json::Map::new();
                m.insert("name".into(), Value::String(l.name.clone()));
                m.insert("color".into(), Value::String(l.color.clone()));
                if let Some(d) = &l.description { m.insert("description".into(), Value::String(d.clone())); }
                if !dry_run {
                    if exists {
                        self.api_in(&["api", &format!("{base}/labels/{}", urlenc(&l.name)),
                                          "-X", "PATCH", "--input", "-"], &Value::Object(m).to_string())?;
                    } else {
                        self.api_in(&["api", &format!("{base}/labels"), "--input", "-"],
                                          &Value::Object(m).to_string())?;
                    }
                }
                out.push(Change { repo: full.clone(), area: "label".into(),
                    detail: format!("{} {}", if exists { "update" } else { "create" }, l.name),
                    applied: !dry_run });
            }
            if want.delete_extra_labels == Some(true) {
                let cur: Value = serde_json::from_str(&self.api(&["api", &format!("{base}/labels?per_page=100")])?)?;
                let keep: std::collections::HashSet<&str> = list.iter().map(|l| l.name.as_str()).collect();
                for x in cur.as_array().cloned().unwrap_or_default() {
                    let n = x["name"].as_str().unwrap_or("").to_string();
                    if !n.is_empty() && !keep.contains(n.as_str()) && !n.starts_with("dynamic/") {
                        if !dry_run {
                            self.api(&["api", &format!("{base}/labels/{}", urlenc(&n)), "-X", "DELETE"])?;
                        }
                        out.push(Change { repo: full.clone(), area: "label".into(),
                            detail: format!("delete extra {n}"), applied: !dry_run });
                    }
                }
            }
        }
        // ---- collaborators ----
        if let Some(list) = &want.collaborators {
            for c in list {
                if !dry_run {
                    let b = serde_json::json!({"permission": c.permission}).to_string();
                    self.api_in(&["api", &format!("{base}/collaborators/{}", c.user),
                                        "-X", "PUT", "--input", "-"], &b)?;
                }
                out.push(Change { repo: full.clone(), area: "collaborator".into(),
                    detail: format!("{} := {}", c.user, c.permission), applied: !dry_run });
            }
        }
        // ---- webhooks ----
        if let Some(list) = &want.webhooks {
            let cur: Value = if dry_run { Value::Null } else {
                serde_json::from_str(&self.api(&["api", &format!("{base}/hooks?per_page=100")])?).unwrap_or(Value::Null)
            };
            for w in list {
                let mut m = serde_json::Map::new();
                m.insert("name".into(), Value::String("web".into()));
                m.insert("active".into(), Value::Bool(w.active.unwrap_or(true)));
                m.insert("events".into(), Value::Array(w.events.iter().map(|e| Value::String(e.clone())).collect()));
                let mut cfg = serde_json::Map::new();
                cfg.insert("url".into(), Value::String(w.url.clone()));
                cfg.insert("content_type".into(), Value::String(w.content_type.clone().unwrap_or_else(|| "json".into())));
                m.insert("config".into(), Value::Object(cfg));
                if !dry_run {
                    self.api_in(&["api", &format!("{base}/hooks"), "--input", "-"],
                                      &Value::Object(m).to_string())?;
                }
                let _ = cur;
                out.push(Change { repo: full.clone(), area: "webhook".into(),
                    detail: format!("create {}", w.url), applied: !dry_run });
            }
        }
        // ---- actions ----
        if let Some(a) = &want.actions {
            if let Some(en) = a.enabled {
                if !dry_run {
                    let b = serde_json::json!({"enabled": en}).to_string();
                    self.api_in(&["api", &format!("{base}/actions/permissions"),
                                        "-X", "PUT", "--input", "-"], &b)?;
                }
                out.push(Change { repo: full.clone(), area: "actions".into(),
                    detail: format!("enabled := {en}"), applied: !dry_run });
            }
        }
        Ok(out)
    }
}
