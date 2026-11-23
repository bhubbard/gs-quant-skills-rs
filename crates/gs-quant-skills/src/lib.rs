//! Pure Rust skill discovery, embedding, and installer library.
//!
//! Replaces `python -m gs_quant.skills` (`list`, `install`, `uninstall`) with a zero-Python,
//! zero-dependency-runtime binary/library that supports both symlinked local directories and
//! compile-time embedded skills.

pub mod embedded;
pub mod error;
pub mod installer;

pub use embedded::{embedded_skills, EmbeddedFile, EmbeddedSkill};
pub use error::SkillError;
pub use installer::{
    discover_skills, install_skills, list_skills, status_in_dir, uninstall_skills,
    AppliedInstallMethod, InstallMode, InstallScope, InstalledSkillRecord, ListedSkill,
    SkillSource, SkillStatus, UninstallOutcome, UninstalledSkillRecord,
};
