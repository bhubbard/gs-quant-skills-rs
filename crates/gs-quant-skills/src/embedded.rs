//! Embedded copy of the `gs-quant-rs` AI skill markdown package.

/// Single embedded file belonging to an embedded skill bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedFile {
    /// File name relative to the skill directory root.
    pub relative_path: &'static str,
    /// UTF-8 file contents.
    pub contents: &'static str,
}

/// Embedded skill package bundled into the binary at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedSkill {
    /// Canonical skill folder name.
    pub name: &'static str,
    /// Files contained inside this skill folder.
    pub files: &'static [EmbeddedFile],
}

const GS_QUANT_RS_FILES: &[EmbeddedFile] = &[
    EmbeddedFile {
        relative_path: "SKILL.md",
        contents: include_str!("../skills/gs-quant-rs/SKILL.md"),
    },
    EmbeddedFile {
        relative_path: "backtesting.md",
        contents: include_str!("../skills/gs-quant-rs/backtesting.md"),
    },
    EmbeddedFile {
        relative_path: "datasets.md",
        contents: include_str!("../skills/gs-quant-rs/datasets.md"),
    },
    EmbeddedFile {
        relative_path: "errors.md",
        contents: include_str!("../skills/gs-quant-rs/errors.md"),
    },
    EmbeddedFile {
        relative_path: "instruments.md",
        contents: include_str!("../skills/gs-quant-rs/instruments.md"),
    },
    EmbeddedFile {
        relative_path: "pricing.md",
        contents: include_str!("../skills/gs-quant-rs/pricing.md"),
    },
    EmbeddedFile {
        relative_path: "results.md",
        contents: include_str!("../skills/gs-quant-rs/results.md"),
    },
    EmbeddedFile {
        relative_path: "risk-measures.md",
        contents: include_str!("../skills/gs-quant-rs/risk-measures.md"),
    },
    EmbeddedFile {
        relative_path: "scenarios.md",
        contents: include_str!("../skills/gs-quant-rs/scenarios.md"),
    },
];

const EMBEDDED_SKILLS: &[EmbeddedSkill] = &[EmbeddedSkill {
    name: "gs-quant-rs",
    files: GS_QUANT_RS_FILES,
}];

/// Returns every compile-time embedded skill bundle.
#[must_use]
pub fn embedded_skills() -> &'static [EmbeddedSkill] {
    EMBEDDED_SKILLS
}
