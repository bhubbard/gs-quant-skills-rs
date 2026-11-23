//! Integration tests for `gs-quant-skills` discovery, installation, and removal.

use std::fs;
use std::path::PathBuf;

use gs_quant_skills::{
    discover_skills, embedded_skills, install_skills, status_in_dir, uninstall_skills,
    AppliedInstallMethod, InstallMode, InstallScope, SkillError, SkillStatus, UninstallOutcome,
};
use tempfile::tempdir;

fn repo_skills_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("skills")
}

#[test]
fn embedded_skills_contain_valid_frontmatter() {
    let embedded = embedded_skills();
    assert_eq!(embedded.len(), 1);
    assert_eq!(embedded[0].name, "gs-quant-rs");
    let skill_md = embedded[0]
        .files
        .iter()
        .find(|f| f.relative_path == "SKILL.md")
        .expect("SKILL.md must exist");
    assert!(skill_md.contents.starts_with("---\n"));
    assert!(skill_md.contents.contains("name: gs-quant-rs"));
    assert_eq!(embedded[0].files.len(), 9);
}

#[test]
fn install_copy_overwrite_and_uninstall_from_embedded() {
    let tmp = tempdir().unwrap();
    let target_root = tmp.path().join(".claude").join("skills");
    let scope = InstallScope::Custom(target_root.clone());

    // Simulate broken symlink / existing file at destination before installing
    fs::create_dir_all(&target_root).unwrap();
    let dest = target_root.join("gs-quant-rs");
    fs::write(&dest, "stale file").unwrap();

    // Pass non-existent explicit skills_dir error check first
    assert!(matches!(
        discover_skills(Some(&tmp.path().join("nope"))),
        Err(SkillError::InvalidSkillsDir { .. })
    ));

    // Install from embedded bundle (using default None while cwd has no ./skills)
    let installed = install_skills(&[], &scope, InstallMode::Copy, None).unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].method, AppliedInstallMethod::Copied);
    assert!(dest.join("SKILL.md").is_file());
    assert!(dest.join("backtesting.md").is_file());
    assert_eq!(
        status_in_dir(&target_root, "gs-quant-rs"),
        SkillStatus::Copied
    );

    // Uninstall existing and then second uninstall skips cleanly
    let removed = uninstall_skills(&[], &scope, None).unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].outcome, UninstallOutcome::Removed);
    assert_eq!(
        status_in_dir(&target_root, "gs-quant-rs"),
        SkillStatus::NotInstalled
    );

    let skipped = uninstall_skills(&[], &scope, None).unwrap();
    assert_eq!(skipped[0].outcome, UninstallOutcome::Skipped);
}

#[cfg(unix)]
#[test]
fn install_symlink_mode_from_disk_skills_dir() {
    let tmp = tempdir().unwrap();
    let target_root = tmp.path().join("skills_target");
    let scope = InstallScope::Custom(target_root.clone());
    let src_dir = repo_skills_dir();

    let installed = install_skills(
        &["gs-quant-rs".into()],
        &scope,
        InstallMode::Auto,
        Some(&src_dir),
    )
    .unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].method, AppliedInstallMethod::Linked);
    assert_eq!(
        status_in_dir(&target_root, "gs-quant-rs"),
        SkillStatus::Linked
    );

    let removed = uninstall_skills(&["gs-quant-rs".into()], &scope, Some(&src_dir)).unwrap();
    assert_eq!(removed[0].outcome, UninstallOutcome::Removed);
    assert_eq!(
        status_in_dir(&target_root, "gs-quant-rs"),
        SkillStatus::NotInstalled
    );
}

#[test]
fn unknown_skill_and_embedded_symlink_errors() {
    let tmp = tempdir().unwrap();
    let scope = InstallScope::Custom(tmp.path().to_path_buf());

    let unknown = install_skills(&["nonexistent".into()], &scope, InstallMode::Copy, None);
    assert!(matches!(unknown, Err(SkillError::UnknownSkills { .. })));

    // Embedded skill cannot be symlinked without an on-disk skills_dir
    let sym = install_skills(&["gs-quant-rs".into()], &scope, InstallMode::Symlink, None);
    if !PathBuf::from("skills").is_dir() {
        assert!(matches!(sym, Err(SkillError::SymlinkRequiresDirectory)));
    }
}
