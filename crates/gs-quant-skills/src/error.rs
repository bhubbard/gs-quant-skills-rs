//! Error types for skill discovery, installation, and removal.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Errors returned by [`crate::installer`] operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SkillError {
    /// No bundled or directory skills were found.
    #[error("no skills found in source `{source_label}`")]
    NoSkillsFound {
        /// Description of the searched source location.
        source_label: String,
    },

    /// One or more requested skill names do not exist.
    #[error("unknown skill(s): {unknown}; available skills: {available}")]
    UnknownSkills {
        /// Comma-separated list of unknown skill names.
        unknown: String,
        /// Comma-separated list of valid skill names.
        available: String,
    },

    /// Scope is required when running non-interactively without a scope flag.
    #[error("scope is required when not running interactively: pass --global, --project, --antigravity, or --target-dir")]
    MissingScope,

    /// Symlink installation was requested from an embedded skill source without a directory on disk.
    #[error("symlink mode requires --skills-dir pointing to an on-disk skill directory")]
    SymlinkRequiresDirectory,

    /// Symlink creation is not supported on this platform.
    #[error("directory symlinks are not supported on this platform; use --copy")]
    SymlinkUnsupported,

    /// Specified skills directory does not exist or is not a directory.
    #[error("invalid skills directory `{path}`")]
    InvalidSkillsDir {
        /// Path passed by caller.
        path: PathBuf,
    },

    /// Unable to resolve user home directory for global scope.
    #[error("could not determine user home directory for global skill scope")]
    MissingHomeDir,

    /// Underlying filesystem error.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}
