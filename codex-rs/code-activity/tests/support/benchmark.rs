//! Shared, versioned synthetic benchmark inputs; programs are never executed.

use codex_code_activity::Language;
use codex_code_activity::Request;
use serde::Deserialize;
use std::fmt::Write;

#[derive(Debug, Deserialize)]
struct Dataset {
    version: u32,
    cases: Vec<Template>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Template {
    id: String,
    language: Language,
    prefix: String,
    body: String,
    suffix: String,
    scales: Vec<usize>,
    effects_per_unit: Option<usize>,
    required_gap: Option<String>,
    #[serde(default)]
    balanced: bool,
}

/// A synthetic input and its hand-reviewed small-scale expectations.
#[derive(Debug)]
pub struct Case {
    /// Stable dataset identity, including its scale.
    pub id: String,
    /// Grammar to use for the inert input.
    pub language: Language,
    /// Deterministically expanded source.
    pub source: String,
    /// Repetition count or named byte/depth boundary.
    pub scale: usize,
    /// Expected operations per unit at small scales, when reviewed.
    pub effects_per_unit: Option<usize>,
    /// Required gap substring for selected abstention cases.
    pub required_gap: Option<String>,
}

impl Case {
    /// Own the request source, matching the analyzer's public boundary.
    pub fn request(&self) -> Request {
        Request::new(
            self.id.as_str().into(),
            self.language,
            self.source.clone(),
            /*cwd*/ None,
        )
    }
}

/// Expand the embedded v1 fixture and deterministic resource-boundary cases.
///
/// # Errors
/// Returns an error if the fixture is invalid or names a different version.
pub fn cases() -> anyhow::Result<Vec<Case>> {
    let dataset: Dataset = serde_json::from_str(include_str!("../fixtures/benchmark-v1.json"))?;
    anyhow::ensure!(
        dataset.version == 1,
        "unsupported benchmark dataset version"
    );
    let mut cases = Vec::new();
    for template in dataset.cases {
        for scale in template.scales {
            let mut source = template.prefix.clone();
            for _ in 0..scale {
                source.push_str(&template.body);
            }
            if template.balanced {
                source.push('0');
                for _ in 0..scale {
                    source.push(')');
                }
            }
            source.push_str(&template.suffix);
            cases.push(Case {
                id: format!("{}-{scale}", template.id),
                language: template.language,
                source,
                scale,
                effects_per_unit: template.effects_per_unit,
                required_gap: template.required_gap.clone(),
            });
        }
    }
    // These exact byte boundaries make rejection costs distinguishable from parsing.
    for scale in [1, 3, 128] {
        for (language, label, declaration) in [
            (Language::Python, "python", "p="),
            (Language::TypeScript, "javascript", "const p="),
        ] {
            cases.push(Case {
                id: format!("{label}-alias-expansion-{scale}"),
                language,
                source: format!(
                    "{declaration}'{}';\n[{}]",
                    "a".repeat(8_000),
                    vec!["p"; scale].join(",")
                ),
                scale,
                effects_per_unit: None,
                required_gap: (scale > 1).then(|| "construction budget".into()),
            });
        }
    }
    for bytes in [1_048_576, 1_048_577] {
        cases.push(Case {
            id: format!("source-byte-boundary-{bytes}"),
            language: Language::Python,
            source: format!("#{}", "x".repeat(bytes - 1)),
            scale: bytes,
            effects_per_unit: None,
            required_gap: None,
        });
    }
    let mut source = "const fs=require('fs');[".to_owned();
    for i in 0..129 {
        if i != 0 {
            source.push(',');
        }
        write!(&mut source, "'file-{i}'")?;
    }
    source.push_str("].forEach(p=>fs.unlinkSync(p));");
    cases.push(Case {
        id: "callback-item-boundary-129".into(),
        language: Language::TypeScript,
        source,
        scale: 129,
        effects_per_unit: None,
        required_gap: Some("iteration bounds".into()),
    });
    Ok(cases)
}
