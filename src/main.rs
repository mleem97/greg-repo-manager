//! greg-repo-manager: profile-based fleet settings for gh/fj/tea.
//! Without subcommand (or `ui`) the Slint wizard starts; otherwise headless CLI.
use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use slint::{Model, ModelRc, VecModel};

use grm_core::backend::{BackendKind, Forge};
use grm_core::engine::apply_fleet;
use grm_core::settings::*;

slint::include_modules!();
mod i18n;
use i18n::I18n;

#[derive(Parser)]
#[command(name = "greg-repo-manager", version)]
struct Cli {
    #[arg(long, default_value = "gh")]
    backend: String,
    #[arg(long, default_value = "mleem97")]
    owner: String,
    /// instance URL/login for fj/tea (empty = CLI default)
    #[arg(long, default_value = "")]
    instance: String,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Launch the Slint wizard (default)
    Ui,
    /// List forge backends + auth state
    Backends,
    /// List repositories
    Repos {
        #[arg(long, default_value_t = 100)] limit: usize,
        #[arg(long, default_value = "")] filter: String,
    },
    /// Show live settings of one repo (JSON)
    Show { repo: String },
    /// Manage profiles
    Profiles {
        #[command(subcommand)]
        op: ProfileOp,
    },
    /// Dry-run: profile x repos
    Diff {
        profile: String,
        #[arg(long)] all: bool,
        #[arg(long, value_delimiter = ',')] repos: Vec<String>,
    },
    /// Apply: profile x repos (needs --yes)
    Apply {
        profile: String,
        #[arg(long)] all: bool,
        #[arg(long, value_delimiter = ',')] repos: Vec<String>,
        #[arg(long)] yes: bool,
    },
}

#[derive(Subcommand)]
enum ProfileOp {
    List,
    Cat { name: String },
    Delete { name: String },
}

fn make_forge(backend: &str, instance: &str) -> Result<Box<dyn Forge>> {
    let inst = if instance.is_empty() { None } else { Some(instance.to_string()) };
    match BackendKind::from_name(backend) {
        Some(BackendKind::GitHub) => Ok(Box::new(grm_core::gh::GhBackend::new()?)),
        Some(BackendKind::Forgejo) => Ok(Box::new(grm_core::fj::FjBackend::new(inst)?)),
        Some(BackendKind::Gitea) => Ok(Box::new(grm_core::tea::TeaBackend::new(inst)?)),
        None => Err(anyhow!("unknown backend '{backend}' (gh|fj|tea)")),
    }
}

fn resolve_repos(forge: &dyn Forge, owner: &str, all: bool, picked: &[String]) -> Result<Vec<String>> {
    if all {
        Ok(forge.list_repos(owner, 500)?.iter().map(|r| r.full_name.clone()).collect())
    } else if picked.is_empty() {
        Err(anyhow!("pass --repos a,b or --all"))
    } else {
        Ok(picked.iter().map(|r| {
            if r.contains('/') { r.clone() } else { format!("{owner}/{r}") }
        }).collect())
    }
}

fn profile_list_model() -> ModelRc<slint::SharedString> {
    let v: Vec<slint::SharedString> = grm_core::profiles::list()
        .unwrap_or_default().iter().map(|s| s.clone().into()).collect();
    ModelRc::from(Rc::new(VecModel::from_slice(&v)))
}

fn run_cli(cli: &Cli) -> Result<()> {
    let cmd = cli.command.as_ref().unwrap();
    let forge = make_forge(&cli.backend, &cli.instance)?;
    match cmd {
        Cmd::Ui => run_ui(cli),
        Cmd::Backends => {
            for name in ["gh", "fj", "tea"] {
                match make_forge(name, &cli.instance) {
                    Ok(f) => match f.check_auth() {
                        Ok(who) => println!("{name}: ok ({who})"),
                        Err(e) => println!("{name}: auth failed ({e})"),
                    },
                    Err(e) => println!("{name}: missing ({e})"),
                }
            }
            Ok(())
        }
        Cmd::Repos { limit, filter } => {
            for r in forge.list_repos(&cli.owner, *limit)? {
                if filter.is_empty() || r.full_name.contains(filter.as_str()) {
                    println!("{} {}", if r.private { "private" } else { "public " }, r.full_name);
                }
            }
            Ok(())
        }
        Cmd::Show { repo } => {
            let s = forge.read_settings(&cli.owner, repo)?;
            println!("{}", serde_json::to_string_pretty(&s)?);
            Ok(())
        }
        Cmd::Profiles { op } => match op {
            ProfileOp::List => {
                for p in grm_core::profiles::list()? { println!("{p}"); }
                Ok(())
            }
            ProfileOp::Cat { name } => {
                println!("{}", toml::to_string_pretty(&grm_core::profiles::load(name)?)?);
                Ok(())
            }
            ProfileOp::Delete { name } => {
                grm_core::profiles::delete(name)?; println!("deleted {name}"); Ok(())
            }
        },
        Cmd::Diff { profile, all, repos } => {
            let p = grm_core::profiles::load(profile)?;
            let targets = resolve_repos(forge.as_ref(), &cli.owner, *all, repos)?;
            let rep = apply_fleet(forge.as_ref(), &cli.owner, &p, &targets, true);
            for (repo, r) in &rep.per_repo {
                match r {
                    Ok(ch) => for c in ch { println!("[dry] {} [{}] {}", repo, c.area, c.detail); },
                    Err(e) => println!("[dry] {repo} ERROR: {e}"),
                }
            }
            Ok(())
        }
        Cmd::Apply { profile, all, repos, yes } => {
            if !yes { return Err(anyhow!("refusing without --yes")); }
            let p = grm_core::profiles::load(profile)?;
            let targets = resolve_repos(forge.as_ref(), &cli.owner, *all, repos)?;
            let rep = apply_fleet(forge.as_ref(), &cli.owner, &p, &targets, false);
            println!("applied {} change(s), {} repo(s) failed",
                     rep.applied_total(), rep.failed_repos().len());
            for f in rep.failed_repos() { println!("FAILED: {f}"); }
            Ok(())
        }
    }
}

// ---------------- UI ----------------

fn settings_from_ui(ui: &AppWindow) -> RepoSettings {
    let nz = |s: slint::SharedString| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    };
    let split_csv = |s: slint::SharedString| -> Vec<String> {
        s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
    };
    let general = GeneralSettings {
        description: nz(ui.get_p_description()),
        homepage: None,
        visibility: nz(ui.get_p_visibility()),
        default_branch: nz(ui.get_p_default_branch()),
        topics: { let t = split_csv(ui.get_p_topics()); if t.is_empty() { None } else { Some(t) } },
        has_issues: Some(ui.get_p_issues()),
        has_wiki: Some(ui.get_p_wiki()),
        has_projects: Some(ui.get_p_projects()),
        has_discussions: None,
        is_template: None,
        archived: Some(ui.get_p_archived()),
        allow_forking: None,
        merge: Some(MergeMethods {
            allow_squash: Some(ui.get_p_squash()),
            allow_merge_commit: Some(ui.get_p_merge_commit()),
            allow_rebase: Some(ui.get_p_rebase()),
            delete_branch_on_merge: Some(ui.get_p_delete_branch()),
        }),
    };
    let branches: Vec<BranchSpec> = split_csv(ui.get_p_branches()).iter().map(|b| BranchSpec {
        name: b.clone(),
        from: None,
        protection: Some(BranchProtection {
            required_checks: { let c = split_csv(ui.get_p_checks()); if c.is_empty() { None } else { Some(c) } },
            strict_up_to_date: None,
            enforce_admins: Some(false),
            required_reviews: if ui.get_p_no_reviews() { Some(0) } else { None },
            dismiss_stale_reviews: None,
            require_code_owner_reviews: None,
            linear_history: None,
            allow_force_pushes: Some(!ui.get_p_no_force()),
            allow_deletions: Some(!ui.get_p_no_delete()),
            lock_branch: None,
            conversation_resolution: None,
        }),
    }).collect();
    let labels: Vec<LabelSpec> = ui.get_p_labels().split(';').filter_map(|e| {
        let mut it = e.splitn(2, ':');
        let n = it.next()?.trim().to_string();
        let c = it.next().unwrap_or("ffffff").trim().to_string();
        if n.is_empty() { None } else { Some(LabelSpec { name: n, color: c, description: None }) }
    }).collect();
    let collaborators: Vec<CollaboratorSpec> = ui.get_p_collaborators().split(';').filter_map(|e| {
        let mut it = e.splitn(2, ':');
        let u = it.next()?.trim().to_string();
        let p = it.next().unwrap_or("push").trim().to_string();
        if u.is_empty() { None } else { Some(CollaboratorSpec { user: u, permission: p }) }
    }).collect();
    RepoSettings {
        general: Some(general),
        ensure_branches: if branches.is_empty() { None } else { Some(branches) },
        delete_branches: None,
        labels: if labels.is_empty() { None } else { Some(labels) },
        delete_extra_labels: Some(ui.get_p_labels_prune()),
        collaborators: if collaborators.is_empty() { None } else { Some(collaborators) },
        webhooks: None,
        actions: Some(ActionsSettings { enabled: Some(ui.get_p_actions()) }),
    }
}

fn run_ui(cli: &Cli) -> Result<()> {
    let lang = std::env::var("LANG").unwrap_or_default();
    let lang = if lang.starts_with("de") { "de" } else { "en" };
    let i18n = I18n::load(lang);
    let ui = AppWindow::new().map_err(|e| anyhow!("slint: {e}"))?;
    let be: Vec<slint::SharedString> = ["gh", "fj", "tea"].iter().map(|s| (*s).into()).collect();
    ui.set_backends(ModelRc::from(Rc::new(VecModel::from_slice(&be))));
    ui.set_backend(cli.backend.clone().into());
    ui.set_owner(cli.owner.clone().into());
    ui.set_instance(cli.instance.clone().into());
    let steps = [
        ("step_backend", "step_backend_sub"), ("step_repos", "step_repos_sub"),
        ("step_profile", "step_profile_sub"), ("step_diff", "step_diff_sub"),
        ("step_apply", "step_apply_sub"), ("step_done", "step_done_sub"),
    ];
    let stepv: Vec<StepDef> = steps.iter().map(|(a, b)| StepDef {
        title: i18n.t(a).into(), sub: i18n.t(b).into(),
    }).collect();
    ui.set_steps(ModelRc::from(Rc::new(VecModel::from_slice(&stepv))));
    let donev = vec![false; 6];
    ui.set_done(ModelRc::from(Rc::new(VecModel::from_slice(&donev))));
    ui.set_profiles(profile_list_model());

    // full repo list cache for the live filter
    let cache: Rc<RefCell<Vec<RepoRow>>> = Rc::new(RefCell::new(vec![]));
    ui.on_filter_edited({
        let ui = ui.as_weak();
        let cache = cache.clone();
        move |f| {
            let ui = ui.unwrap();
            let f = f.to_string();
            let rows: Vec<RepoRow> = cache.borrow().iter()
                .filter(|r| f.is_empty() || r.name.contains(f.as_str()))
                .cloned().collect();
            ui.set_repos(ModelRc::from(Rc::new(VecModel::from_slice(&rows))));
        }
    });

    let show_caps = |ui: &AppWindow, backend: &str, instance: &str| {
        let caps: Vec<CapRow> = match make_forge(backend, instance) {
            Ok(f) => {
                let c = f.capabilities();
                [("list_repos", c.list_repos), ("read_settings", c.read_settings),
                 ("write_general", c.write_general), ("branch_protection", c.branch_protection),
                 ("manage_branches", c.manage_branches), ("manage_labels", c.manage_labels),
                 ("manage_collaborators", c.manage_collaborators), ("manage_webhooks", c.manage_webhooks),
                 ("manage_actions", c.manage_actions), ("manage_releases", c.manage_releases)]
                .into_iter().map(|(n, ok)| CapRow { name: n.into(), ok }).collect()
            }
            Err(_) => vec![],
        };
        ui.set_caps(ModelRc::from(Rc::new(VecModel::from_slice(&caps))));
    };
    show_caps(&ui, &cli.backend, &cli.instance);

    ui.on_nav_to({
        let ui = ui.as_weak();
        move |i| { ui.unwrap().set_current(i); }
    });
    ui.on_backend_test({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let b = ui.get_backend().to_string();
            let inst = ui.get_instance().to_string();
            let msg = match make_forge(&b, &inst) {
                Ok(f) => match f.check_auth() {
                    Ok(who) => format!("OK: {b} als {who}"),
                    Err(e) => format!("Auth-Fehler: {e}"),
                },
                Err(e) => format!("Backend fehlt: {e}"),
            };
            ui.set_auth_state(msg.into());
            show_caps(&ui, &b, &inst);
        }
    });

    ui.on_repos_refresh({
        let ui = ui.as_weak();
        let cache = cache.clone();
        move || {
            let ui = ui.unwrap();
            let b = ui.get_backend().to_string();
            let owner = ui.get_owner().to_string();
            let inst = ui.get_instance().to_string();
            match make_forge(&b, &inst).and_then(|f| f.list_repos(&owner, 500).map_err(anyhow::Error::from)) {
                Ok(list) => {
                    let rows: Vec<RepoRow> = list.iter().map(|r| RepoRow {
                        name: r.full_name.clone().into(),
                        checked: true,
                        note: (if r.private { "private" } else { "public" }).into(),
                    }).collect();
                    *cache.borrow_mut() = rows.clone();
                    ui.set_repos(ModelRc::from(Rc::new(VecModel::from_slice(&rows))));
                }
                Err(e) => {
                    let rows = vec![RepoRow {
                        name: format!("FEHLER: {e}").into(), checked: false, note: "".into(),
                    }];
                    ui.set_repos(ModelRc::from(Rc::new(VecModel::from_slice(&rows))));
                }
            }
        }
    });
    ui.on_repos_toggle({
        let ui = ui.as_weak();
        let cache = cache.clone();
        move |i| {
            let ui = ui.unwrap();
            let m = ui.get_repos();
            if let Some(vm) = m.as_any().downcast_ref::<VecModel<RepoRow>>() {
                if let Some(mut row) = vm.row_data(i as usize) {
                    row.checked = !row.checked;
                    vm.set_row_data(i as usize, row.clone());
                    let name = row.name.to_string();
                    for c in cache.borrow_mut().iter_mut() {
                        if c.name == name { c.checked = row.checked; }
                    }
                }
            }
        }
    });
    ui.on_repos_all({
        let ui = ui.as_weak();
        let cache = cache.clone();
        move |v| {
            let ui = ui.unwrap();
            for c in cache.borrow_mut().iter_mut() { c.checked = v; }
            let rows: Vec<RepoRow> = cache.borrow().iter().filter(|r| {
                ui.get_repo_filter().is_empty() || r.name.contains(ui.get_repo_filter().as_str())
            }).cloned().collect();
            ui.set_repos(ModelRc::from(Rc::new(VecModel::from_slice(&rows))));
        }
    });
    ui.on_profile_load({
        let ui = ui.as_weak();
        move |name| {
            let ui = ui.unwrap();
            let name = name.trim().to_string();
            let Ok(p) = grm_core::profiles::load(&name) else { return; };
            ui.set_profile_desc(p.description.unwrap_or_default().into());
            if let Some(g) = p.settings.general {
                if let Some(v) = g.description { ui.set_p_description(v.into()); }
                if let Some(v) = g.visibility { ui.set_p_visibility(v.into()); }
                if let Some(v) = g.default_branch { ui.set_p_default_branch(v.into()); }
                if let Some(v) = g.topics { ui.set_p_topics(v.join(", ").into()); }
                if let Some(v) = g.has_issues { ui.set_p_issues(v); }
                if let Some(v) = g.has_wiki { ui.set_p_wiki(v); }
                if let Some(v) = g.has_projects { ui.set_p_projects(v); }
                if let Some(v) = g.archived { ui.set_p_archived(v); }
                if let Some(m) = g.merge {
                    if let Some(v) = m.allow_squash { ui.set_p_squash(v); }
                    if let Some(v) = m.allow_merge_commit { ui.set_p_merge_commit(v); }
                    if let Some(v) = m.allow_rebase { ui.set_p_rebase(v); }
                    if let Some(v) = m.delete_branch_on_merge { ui.set_p_delete_branch(v); }
                }
            }
            if let Some(br) = p.settings.ensure_branches {
                ui.set_p_branches(br.iter().map(|b| b.name.clone()).collect::<Vec<_>>().join(", ").into());
            }
            if let Some(lb) = p.settings.labels {
                ui.set_p_labels(lb.iter().map(|l| format!("{}:{}", l.name, l.color)).collect::<Vec<_>>().join(";").into());
            }
            if let Some(v) = p.settings.delete_extra_labels { ui.set_p_labels_prune(v); }
            if let Some(a) = p.settings.actions {
                if let Some(v) = a.enabled { ui.set_p_actions(v); }
            }
        }
    });
    ui.on_profile_save({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let name = ui.get_profile_name().trim().to_string();
            if name.is_empty() { return; }
            let mut p = Profile::new(name);
            p.description = Some(ui.get_profile_desc().to_string());
            p.backend = Some(ui.get_backend().to_string());
            p.settings = settings_from_ui(&ui);
            match grm_core::profiles::save(&p) {
                Ok(path) => ui.set_summary(format!("Profil gespeichert: {}", path.display()).into()),
                Err(e) => ui.set_summary(format!("Fehler: {e}").into()),
            }
            ui.set_profiles(profile_list_model());
        }
    });
    ui.on_profile_delete({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let name = ui.get_profile_name().trim().to_string();
            match grm_core::profiles::delete(&name) {
                Ok(()) => ui.set_summary(format!("Profil gelöscht: {name}").into()),
                Err(e) => ui.set_summary(format!("Fehler: {e}").into()),
            }
            ui.set_profiles(profile_list_model());
        }
    });
    let do_run = {
        let ui = ui.as_weak();
        let cache = cache.clone();
        move |dry: bool| {
            let ui = ui.unwrap();
            let b = ui.get_backend().to_string();
            let owner = ui.get_owner().to_string();
            let inst = ui.get_instance().to_string();
            let targets: Vec<String> = cache.borrow().iter()
                .filter(|r| r.checked).map(|r| r.name.to_string()).collect();
            let settings = settings_from_ui(&ui);
            let profile = Profile { name: ui.get_profile_name().to_string(), description: None, backend: None, settings };
            let n = targets.len();
            ui.set_busy(true);
            ui.set_progress(0.0);
            let weak = ui.as_weak();
            std::thread::spawn(move || {
                let result = make_forge(&b, &inst).map(|f| {
                    apply_fleet(f.as_ref(), &owner, &profile, &targets, dry)
                });
                weak.upgrade_in_event_loop(move |ui| {
                    match result {
                        Ok(rep) => {
                            let rows: Vec<ChangeRow> = rep.per_repo.iter().flat_map(|(repo, r)| match r {
                                Ok(ch) => ch.iter().map(|c| ChangeRow {
                                    repo: repo.clone().into(), area: c.area.clone().into(),
                                    detail: c.detail.clone().into(),
                                    applied: if c.applied { "ja".into() } else { "dry".into() },
                                }).collect::<Vec<_>>(),
                                Err(e) => vec![ChangeRow {
                                    repo: repo.clone().into(), area: "FEHLER".into(),
                                    detail: e.clone().into(), applied: "nein".into(),
                                }],
                            }).collect();
                            let total = rows.len();
                            ui.set_changes(ModelRc::from(Rc::new(VecModel::from_slice(&rows))));
                            ui.set_summary(format!("{n} Repos, {total} Einträge (dry_run={dry})").into());
                            ui.set_log(format!("{n} Repos verarbeitet.").into());
                        }
                        Err(e) => { ui.set_summary(format!("Backend-Fehler: {e}").into()); }
                    }
                    ui.set_busy(false);
                    ui.set_progress(1.0);
                    let mut done: Vec<bool> = ui.get_done().iter().collect();
                    if done.len() == 6 { done[if dry { 3 } else { 4 }] = true;
                        ui.set_done(ModelRc::from(Rc::new(VecModel::from_slice(&done)))); }
                }).ok();
            });
        }
    };
    ui.on_run_diff({
        let do_run = do_run.clone();
        move || do_run(true)
    });
    ui.on_run_apply({
        let do_run = do_run.clone();
        move || do_run(false)
    });
    ui.on_quit_app(|| { std::process::exit(0); });
    ui.run().map_err(|e| anyhow!("slint run: {e}"))
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let cli = Cli::parse();
    if cli.command.is_none() {
        return run_ui(&cli);
    }
    run_cli(&cli)
}
