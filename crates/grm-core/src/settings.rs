//! Repo settings model: every field is `Option` so profiles can be partial
//! (only `Some` values are applied).

use serde::{Deserialize, Serialize};

/// Merge method toggles on a repository.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergeMethods {
    pub allow_squash: Option<bool>,
    pub allow_merge_commit: Option<bool>,
    pub allow_rebase: Option<bool>,
    pub delete_branch_on_merge: Option<bool>,
}

/// General repository properties.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub visibility: Option<String>, // public | private
    pub default_branch: Option<String>,
    pub topics: Option<Vec<String>>,
    pub has_issues: Option<bool>,
    pub has_wiki: Option<bool>,
    pub has_projects: Option<bool>,
    pub has_discussions: Option<bool>,
    pub is_template: Option<bool>,
    pub archived: Option<bool>,
    pub allow_forking: Option<bool>,
    pub merge: Option<MergeMethods>,
}

/// Classic branch protection for one branch (reviews always optional here).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BranchProtection {
    pub required_checks: Option<Vec<String>>,
    pub strict_up_to_date: Option<bool>,
    pub enforce_admins: Option<bool>,
    pub required_reviews: Option<u32>,
    pub dismiss_stale_reviews: Option<bool>,
    pub require_code_owner_reviews: Option<bool>,
    pub linear_history: Option<bool>,
    pub allow_force_pushes: Option<bool>,
    pub allow_deletions: Option<bool>,
    pub lock_branch: Option<bool>,
    pub conversation_resolution: Option<bool>,
}

/// A branch to ensure plus optional protection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BranchSpec {
    pub name: String,
    pub from: Option<String>, // source ref when creating
    pub protection: Option<BranchProtection>,
}

/// Label definition for label sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSpec {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

/// Collaborator entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaboratorSpec {
    pub user: String,
    pub permission: String, // pull | triage | push | maintain | admin
}

/// Webhook entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSpec {
    pub url: String,
    pub events: Vec<String>,
    pub active: Option<bool>,
    pub content_type: Option<String>, // json | form
}

/// Actions permissions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionsSettings {
    pub enabled: Option<bool>,
}

/// Full per-repo desired state. `None` = leave untouched.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoSettings {
    pub general: Option<GeneralSettings>,
    pub ensure_branches: Option<Vec<BranchSpec>>,
    pub delete_branches: Option<Vec<String>>,
    pub labels: Option<Vec<LabelSpec>>,
    pub delete_extra_labels: Option<bool>,
    pub collaborators: Option<Vec<CollaboratorSpec>>,
    pub webhooks: Option<Vec<WebhookSpec>>,
    pub actions: Option<ActionsSettings>,
}

/// A named, stored profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub description: Option<String>,
    pub backend: Option<String>, // gh | fj | tea (unset = any)
    pub settings: RepoSettings,
}

impl Profile {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), description: None, backend: None, settings: RepoSettings::default() }
    }
}
