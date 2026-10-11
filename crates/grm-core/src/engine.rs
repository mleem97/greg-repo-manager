//! Fleet apply engine: profile x repos -> per-repo changes.

use anyhow::Result;

use crate::backend::{Change, Forge};
use crate::settings::Profile;

pub struct ApplyReport {
    pub profile: String,
    pub dry_run: bool,
    pub per_repo: Vec<(String, Result<Vec<Change>, String>)>,
}

impl ApplyReport {
    pub fn applied_total(&self) -> usize {
        self.per_repo.iter().map(|(_, r)| {
            r.as_ref().map(|v| v.iter().filter(|c| c.applied).count()).unwrap_or(0)
        }).sum()
    }
    pub fn failed_repos(&self) -> Vec<&str> {
        self.per_repo.iter().filter(|(_, r)| r.is_err()).map(|(n, _)| n.as_str()).collect()
    }
}

/// Apply `profile` to every repo in `repos` (each "owner/name").
pub fn apply_fleet<F: Forge + ?Sized>(forge: &F, owner: &str, profile: &Profile, repos: &[String], dry_run: bool) -> ApplyReport {
    let mut per_repo = vec![];
    for full in repos {
        let name = full.strip_prefix(&format!("{owner}/")).unwrap_or(full).to_string();
        let r = forge.apply(owner, &name, &profile.settings, dry_run).map_err(|e| e.to_string());
        per_repo.push((full.clone(), r));
    }
    ApplyReport { profile: profile.name.clone(), dry_run, per_repo }
}
