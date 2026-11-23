//! Skill discovery, symlink/copy installation, status reporting, and uninstallation.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::embedded::{embedded_skills, EmbeddedSkill};
use crate::error::SkillError;

/// Target directory scope for installing or uninstalling AI agent skills.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InstallScope {
    /// Global Claude Code directory (`~/.claude/skills`).
    Global,
    /// Project-local Claude Code directory (`./.claude/skills`).
    Project,
    /// Global Antigravity skills directory (`~/.gemini/config/skills`).
    Antigravity,
    /// Explicit caller-supplied directory path.
    Custom(PathBuf),
}

impl InstallScope {
    /// Resolves the target skills directory without creating it on disk.
    ///
    /// # Errors
    ///
    /// Returns [`SkillError::MissingHomeDir`] if a home-relative scope is selected and the OS
    /// home directory cannot be resolved, or [`SkillError::Io`] if `current_dir` fails.
    pub fn resolve_target_dir(&self) -> Result<PathBuf, SkillError> {
        match self {
            Self::Global => {
                let home = dirs::home_dir().ok_or(SkillError::MissingHomeDir)?;
                Ok(home.join(".claude").join("skills"))
            }
            Self::Project => {
                let cwd = std::env::current_dir()?;
                Ok(cwd.join(".claude").join("skills"))
            }
            Self::Antigravity => {
                let home = dirs::home_dir().ok_or(SkillError::MissingHomeDir)?;
                Ok(home.join(".gemini").join("config").join("skills"))
            }
            Self::Custom(path) => Ok(path.clone()),
        }
    }
}

/// Strategy used to place a skill into the target skills directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InstallMode {
    /// Symlink on Unix when installing from an on-disk directory; copy otherwise.
    #[default]
    Auto,
    /// Require a directory symlink (`Err` if source is embedded or platform lacks directory symlinks).
    Symlink,
    /// Always copy/materialize files into the destination directory.
    Copy,
}

/// Observed installation status for one skill at one scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SkillStatus {
    /// Installed via symlink.
    Linked,
    /// Installed as a copied directory.
    Copied,
    /// Not present at the scope path.
    NotInstalled,
}

impl SkillStatus {
    /// Short lowercase status label matching `gs_quant.skills list`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linked => "linked",
            Self::Copied => "copied",
            Self::NotInstalled => "not installed",
        }
    }
}

impl fmt::Display for SkillStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source from which a skill can be installed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkillSource {
    /// Directory on disk containing `SKILL.md` and supporting guides.
    Directory(PathBuf),
    /// Compile-time embedded bundle inside the binary.
    Embedded(&'static EmbeddedSkill),
}

/// Summary of discovered skills and their installation state across scopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedSkill {
    /// Skill name (directory name).
    pub name: String,
    /// Installation status in `~/.claude/skills`.
    pub global: SkillStatus,
    /// Installation status in `./.claude/skills`.
    pub project: SkillStatus,
    /// Installation status in `~/.gemini/config/skills`.
    pub antigravity: SkillStatus,
}

/// Method used when installing a single skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AppliedInstallMethod {
    /// Created a directory symlink.
    Linked,
    /// Copied or wrote files into a directory.
    Copied,
}

impl AppliedInstallMethod {
    /// Verb printed by `install`, matching `gs_quant.skills`.
    #[must_use]
    pub fn verb(self) -> &'static str {
        match self {
            Self::Linked => "linked",
            Self::Copied => "copied",
        }
    }
}

/// Result entry for one installed skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledSkillRecord {
    /// Installed skill name.
    pub name: String,
    /// Whether the skill was linked or copied.
    pub method: AppliedInstallMethod,
    /// Final destination path.
    pub target: PathBuf,
}

/// Outcome of uninstalling a single skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UninstallOutcome {
    /// Removed an existing link, file, or directory.
    Removed,
    /// Nothing existed at the destination path.
    Skipped,
}

/// Result entry for one uninstalled skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstalledSkillRecord {
    /// Skill name.
    pub name: String,
    /// Whether removal occurred or was skipped.
    pub outcome: UninstallOutcome,
    /// Target path checked.
    pub target: PathBuf,
}

/// Discovers skills from `skills_dir` when provided, or from `./skills` if present on disk,
/// falling back to the compile-time embedded bundle otherwise.
///
/// # Errors
///
/// Returns [`SkillError::InvalidSkillsDir`] if `skills_dir` was given explicitly and is not an
/// existing directory, or [`SkillError::Io`] on filesystem traversal failure.
pub fn discover_skills(
    skills_dir: Option<&Path>,
) -> Result<(String, BTreeMap<String, SkillSource>), SkillError> {
    if let Some(dir) = skills_dir {
        if !dir.is_dir() {
            return Err(SkillError::InvalidSkillsDir {
                path: dir.to_path_buf(),
            });
        }
        let map = scan_directory(dir)?;
        return Ok((dir.display().to_string(), map));
    }

    let local_skills = Path::new("skills");
    if local_skills.is_dir() {
        let map = scan_directory(local_skills)?;
        if !map.is_empty() {
            let abs = fs::canonicalize(local_skills).unwrap_or_else(|_| local_skills.to_path_buf());
            return Ok((abs.display().to_string(), map));
        }
    }

    let mut embedded = BTreeMap::new();
    for skill in embedded_skills() {
        embedded.insert(skill.name.to_owned(), SkillSource::Embedded(skill));
    }
    Ok(("embedded".to_owned(), embedded))
}

fn scan_directory(dir: &Path) -> Result<BTreeMap<String, SkillSource>, SkillError> {
    let mut skills = BTreeMap::new();
    for entry_res in fs::read_dir(dir)? {
        let entry = entry_res?;
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                skills.insert(name.to_owned(), SkillSource::Directory(path));
            }
        }
    }
    Ok(skills)
}

/// Checks installation status (`Linked`, `Copied`, or `NotInstalled`) at a single target root.
#[must_use]
pub fn status_in_dir(target_root: &Path, skill_name: &str) -> SkillStatus {
    let path = target_root.join(skill_name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => SkillStatus::Linked,
        Ok(_) => SkillStatus::Copied,
        Err(_) => SkillStatus::NotInstalled,
    }
}

/// Lists all available skills along with their status across global, project, and Antigravity scopes.
///
/// # Errors
///
/// Returns [`SkillError`] if `skills_dir` is invalid or I/O fails.
pub fn list_skills(skills_dir: Option<&Path>) -> Result<(String, Vec<ListedSkill>), SkillError> {
    let (source_label, available) = discover_skills(skills_dir)?;
    let global_dir = InstallScope::Global.resolve_target_dir().ok();
    let project_dir = InstallScope::Project.resolve_target_dir().ok();
    let agy_dir = InstallScope::Antigravity.resolve_target_dir().ok();

    let mut rows = Vec::with_capacity(available.len());
    for name in available.keys() {
        let global = global_dir
            .as_deref()
            .map_or(SkillStatus::NotInstalled, |d| status_in_dir(d, name));
        let project = project_dir
            .as_deref()
            .map_or(SkillStatus::NotInstalled, |d| status_in_dir(d, name));
        let antigravity = agy_dir
            .as_deref()
            .map_or(SkillStatus::NotInstalled, |d| status_in_dir(d, name));
        rows.push(ListedSkill {
            name: name.clone(),
            global,
            project,
            antigravity,
        });
    }
    Ok((source_label, rows))
}

/// Installs all skills (when `requested` is empty) or the named subset into `scope`.
///
/// Overwrites any existing symlink, file, or directory at the destination.
///
/// # Errors
///
/// Returns [`SkillError::NoSkillsFound`], [`SkillError::UnknownSkills`],
/// [`SkillError::SymlinkRequiresDirectory`], [`SkillError::SymlinkUnsupported`], or [`SkillError::Io`].
pub fn install_skills(
    requested: &[String],
    scope: &InstallScope,
    mode: InstallMode,
    skills_dir: Option<&Path>,
) -> Result<Vec<InstalledSkillRecord>, SkillError> {
    let (source_label, available) = discover_skills(skills_dir)?;
    if available.is_empty() {
        return Err(SkillError::NoSkillsFound { source_label });
    }
    let names = resolve_skill_names(requested, &available)?;
    let target_dir = scope.resolve_target_dir()?;
    fs::create_dir_all(&target_dir)?;

    let mut records = Vec::with_capacity(names.len());
    for name in names {
        let source = available
            .get(&name)
            .ok_or_else(|| SkillError::UnknownSkills {
                unknown: name.clone(),
                available: available.keys().cloned().collect::<Vec<_>>().join(", "),
            })?;
        let target = target_dir.join(&name);
        let method = install_one(source, &target, mode)?;
        records.push(InstalledSkillRecord {
            name,
            method,
            target,
        });
    }
    Ok(records)
}

/// Uninstalls all known skills (when `requested` is empty) or the named subset from `scope`.
///
/// # Errors
///
/// Returns [`SkillError::UnknownSkills`] if any requested skill name does not exist, or
/// [`SkillError::Io`] on removal failure.
pub fn uninstall_skills(
    requested: &[String],
    scope: &InstallScope,
    skills_dir: Option<&Path>,
) -> Result<Vec<UninstalledSkillRecord>, SkillError> {
    let (_label, available) = discover_skills(skills_dir)?;
    let names = resolve_skill_names(requested, &available)?;
    let target_dir = scope.resolve_target_dir()?;

    let mut records = Vec::with_capacity(names.len());
    for name in names {
        let target = target_dir.join(&name);
        let outcome = if fs::symlink_metadata(&target).is_ok() {
            remove_existing(&target)?;
            UninstallOutcome::Removed
        } else {
            UninstallOutcome::Skipped
        };
        records.push(UninstalledSkillRecord {
            name,
            outcome,
            target,
        });
    }
    Ok(records)
}

fn resolve_skill_names(
    requested: &[String],
    available: &BTreeMap<String, SkillSource>,
) -> Result<Vec<String>, SkillError> {
    if requested.is_empty() {
        return Ok(available.keys().cloned().collect());
    }
    let unknown: Vec<&str> = requested
        .iter()
        .map(String::as_str)
        .filter(|name| !available.contains_key(*name))
        .collect();
    if !unknown.is_empty() {
        let avail = if available.is_empty() {
            "(none)".to_owned()
        } else {
            available.keys().cloned().collect::<Vec<_>>().join(", ")
        };
        return Err(SkillError::UnknownSkills {
            unknown: unknown.join(", "),
            available: avail,
        });
    }
    Ok(requested.to_vec())
}

fn remove_existing(target: &Path) -> Result<(), SkillError> {
    let meta = fs::symlink_metadata(target)?;
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(target)?;
    } else if meta.is_dir() {
        fs::remove_dir_all(target)?;
    }
    Ok(())
}

fn install_one(
    source: &SkillSource,
    target: &Path,
    mode: InstallMode,
) -> Result<AppliedInstallMethod, SkillError> {
    let should_symlink = match (mode, source) {
        (InstallMode::Copy, _) => false,
        (InstallMode::Symlink, SkillSource::Embedded(_)) => {
            return Err(SkillError::SymlinkRequiresDirectory);
        }
        (InstallMode::Symlink, SkillSource::Directory(_)) => {
            if cfg!(unix) {
                true
            } else {
                return Err(SkillError::SymlinkUnsupported);
            }
        }
        (InstallMode::Auto, SkillSource::Directory(_)) => cfg!(unix),
        (InstallMode::Auto, SkillSource::Embedded(_)) => false,
    };

    if fs::symlink_metadata(target).is_ok() {
        remove_existing(target)?;
    }

    if should_symlink {
        if let SkillSource::Directory(src_dir) = source {
            let abs_src = fs::canonicalize(src_dir).unwrap_or_else(|_| src_dir.clone());
            create_dir_symlink(&abs_src, target)?;
            return Ok(AppliedInstallMethod::Linked);
        }
    }

    match source {
        SkillSource::Directory(src_dir) => {
            copy_dir_recursive(src_dir, target)?;
        }
        SkillSource::Embedded(bundle) => {
            fs::create_dir_all(target)?;
            for file in bundle.files {
                let dest = target.join(file.relative_path);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(dest, file.contents)?;
            }
        }
    }
    Ok(AppliedInstallMethod::Copied)
}

#[cfg(unix)]
fn create_dir_symlink(src: &Path, dst: &Path) -> Result<(), SkillError> {
    std::os::unix::fs::symlink(src, dst)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_dir_symlink(_src: &Path, _dst: &Path) -> Result<(), SkillError> {
    Err(SkillError::SymlinkUnsupported)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), SkillError> {
    fs::create_dir_all(dst)?;
    for entry_res in fs::read_dir(src)? {
        let entry = entry_res?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
