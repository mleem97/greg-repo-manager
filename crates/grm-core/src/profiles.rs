//! Profile store: TOML files under ~/.config/greg-repo-manager/profiles/.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::settings::Profile;

fn dir() -> Result<PathBuf> {
    let mut d = dirs::config_dir().context("no config dir")?;
    d.push("greg-repo-manager");
    d.push("profiles");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn list() -> Result<Vec<String>> {
    let d = dir()?;
    let mut out = vec![];
    for e in std::fs::read_dir(d)? {
        let e = e?;
        if e.path().extension().map(|x| x == "toml").unwrap_or(false) {
            if let Some(s) = e.path().file_stem().and_then(|s| s.to_str()) {
                out.push(s.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

pub fn load(name: &str) -> Result<Profile> {
    let mut p = dir()?;
    p.push(format!("{name}.toml"));
    let t = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    toml::from_str(&t).context("parse profile")
}

pub fn save(profile: &Profile) -> Result<PathBuf> {
    let mut p = dir()?;
    p.push(format!("{}.toml", profile.name));
    std::fs::write(&p, toml::to_string_pretty(profile)?)?;
    Ok(p)
}

pub fn delete(name: &str) -> Result<()> {
    let mut p = dir()?;
    p.push(format!("{name}.toml"));
    std::fs::remove_file(&p)?;
    Ok(())
}
