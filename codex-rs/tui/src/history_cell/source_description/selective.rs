//! Conditional predictions share the live/restored cell; no file-change badges.

mod javascript;
mod logical_stream;
mod newline;
mod recipes;
mod snapshot;
mod summary;

use recipes::recipe_lines;

use codex_protocol::source_description::SelectiveBarrier;
use codex_protocol::source_description::SelectiveBarrierReason;
use codex_protocol::source_description::SelectiveCarrierBasis;
use codex_protocol::source_description::SelectiveEditBasis;
use codex_protocol::source_description::SelectiveEditDetails;
use codex_protocol::source_description::SelectiveLaterContext;
use codex_protocol::source_description::SelectivePredictedEdit;
use codex_protocol::source_description::SelectivePythonAssumption;
use codex_protocol::source_description::SelectivePythonBarrierReason;
use codex_protocol::source_description::SelectivePythonDependency;
use codex_protocol::source_description::SelectivePythonFinalState;
use codex_protocol::source_description::SelectivePythonOperation;
use codex_protocol::source_description::SelectiveTarget;
use codex_protocol::source_description::SelectiveTextEncoding;
use codex_protocol::source_description::SourceDescriptionLanguage;
use codex_protocol::source_description::SourceDescriptionSpan;
use ratatui::style::Stylize as _;
use ratatui::text::Line;

#[derive(Debug)]
pub(super) struct SelectiveCell {
    pub(super) edits: Vec<SelectivePredictedEdit>,
    pub(super) barriers: Vec<SelectiveBarrier>,
}

impl SelectiveCell {
    pub(super) fn summary(&self) -> String {
        format!(
            "Predicted edits · {} operations · {} unresolved regions",
            self.edits.len(),
            self.barriers.len()
        )
    }

    pub(super) fn compact(&self) -> Vec<Line<'static>> {
        summary::compact(self)
    }

    pub(super) fn detail(&self) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from(self.summary()).dim(),
            Line::from("Conditional forecast; execution and final file state are unobserved.")
                .dim(),
        ];
        if self.edits.is_empty() {
            lines.push(Line::from("No supported prediction was retained; this does not establish that the source makes no edits.").dim());
        }
        for edit in &self.edits {
            lines.push(Line::default());
            lines.push(
                Line::from(format!(
                    "Predicted operation {} · {}",
                    edit.ordinal + 1,
                    location(&edit.evidence)
                ))
                .bold(),
            );
            lines.push(Line::from(match edit.basis {
                SelectiveEditBasis::StandardApiForecast => "Basis: standard API forecast",
                SelectiveEditBasis::HostToolRequest => "Basis: expected host-tool request",
            }));
            for condition in &edit.predecessor_conditions {
                lines.push(Line::from(format!(
                    "Requires earlier invocation to complete normally · {}",
                    location(condition)
                )));
            }
            for carrier in &edit.carrier_assumptions {
                let basis = match carrier.basis {
                    SelectiveCarrierBasis::StandardShellDispatchAndStartup => {
                        "standard shell dispatch and startup"
                    }
                    SelectiveCarrierBasis::StandardPython3Cli => "Python 3 CLI semantics",
                    SelectiveCarrierBasis::StandardNodeCommonJsCli => "Node CommonJS CLI semantics",
                };
                let language = match carrier.language {
                    SourceDescriptionLanguage::TypeScript => "TypeScript",
                    SourceDescriptionLanguage::Shell => "shell",
                    SourceDescriptionLanguage::Python => "Python",
                };
                lines.push(Line::from(format!(
                    "Assume {basis} for {language} · {}",
                    location(&carrier.invocation)
                )));
            }
            match &edit.details {
                SelectiveEditDetails::Patch { patch, cwd } => {
                    lines.push(Line::from(format!(
                        "Conditional patch request · cwd {}",
                        cwd_label(cwd.as_deref())
                    )));
                    lines.push(Line::from("Requested patch text:").bold());
                    lines.extend(
                        patch
                            .payload
                            .split_terminator('\n')
                            .map(|line| Line::from(line.to_owned())),
                    );
                }
                SelectiveEditDetails::JavaScript {
                    prediction,
                    assumptions,
                } => {
                    javascript::detail(prediction, assumptions, &mut lines);
                }
                SelectiveEditDetails::Python {
                    prediction,
                    assumptions,
                } => {
                    lines.push(Line::from(format!(
                        "Target: {}",
                        target_label(&prediction.target)
                    )));
                    for iteration in &prediction.iteration_context {
                        lines.push(Line::from(format!(
                            "Literal loop iteration {} of {} · {}",
                            iteration.index + 1,
                            iteration.literal_count,
                            location(&iteration.evidence)
                        )));
                    }
                    match &prediction.operation {
                        SelectivePythonOperation::Truncate => {
                            lines.push(Line::from("Operation: truncate"))
                        }
                        SelectivePythonOperation::WriteText {
                            content,
                            encoding,
                            newline,
                        } => {
                            lines.push(Line::from(format!(
                                "Operation: write text · encoding {}",
                                encoding_label(*encoding)
                            )));
                            if let Some(newline) = newline {
                                lines.push(Line::from(format!(
                                    "Source output newline: {}",
                                    newline::write_label(*newline)
                                )));
                            }
                            recipe_lines(content, prediction.evidence.source_id, "  ", &mut lines);
                        }
                        SelectivePythonOperation::LogicalWrite(write) => {
                            logical_stream::operation(
                                write,
                                prediction.evidence.source_id,
                                &mut lines,
                            );
                        }
                    }
                    for assumption in assumptions {
                        lines.push(Line::from(format!(
                            "Assume {}",
                            assumption_label(*assumption)
                        )));
                    }
                    for condition in &prediction.predecessor_conditions {
                        lines.push(Line::from(format!(
                            "Requires earlier Python operation to complete normally · {}",
                            location(condition)
                        )));
                    }
                    for dependency in &prediction.dependencies {
                        let value = match dependency {
                            SelectivePythonDependency::Binding {
                                name,
                                version,
                                evidence,
                            } => {
                                format!(
                                    "binding {name:?} version {version} · {}",
                                    location(evidence)
                                )
                            }
                            SelectivePythonDependency::ReadValue { read_id, evidence } => {
                                format!(
                                    "read {}:{read_id} · {}",
                                    evidence.source_id,
                                    location(evidence)
                                )
                            }
                            SelectivePythonDependency::EntryWorkingDirectory { cwd } => {
                                format!("entry cwd {}", cwd_label(cwd.as_deref()))
                            }
                            SelectivePythonDependency::Capability { assumption } => {
                                assumption_label(*assumption).to_owned()
                            }
                            SelectivePythonDependency::FilesystemEpoch { epoch } => {
                                format!(
                                    "filesystem epoch {epoch} within source {}",
                                    prediction.evidence.source_id
                                )
                            }
                        };
                        lines.push(Line::from(format!("Depends on {value}")));
                    }
                    lines.push(Line::from(match prediction.final_state {
                        SelectivePythonFinalState::NotEstablished => "Python suffix: final state is unobserved.",
                        SelectivePythonFinalState::KnownLaterTouch => "Python suffix: a later modeled operation touches this path spelling.",
                        SelectivePythonFinalState::PossibleAliasLaterTouch => "Python suffix: a later modeled operation may touch an alias.",
                        SelectivePythonFinalState::OpaqueSuccessor => "Python suffix: unsupported code may change the outcome.",
                    }).dim());
                }
            }
            if let Some(preview) = &edit.snapshot {
                snapshot::detail(preview, &mut lines);
            }
            lines.push(
                Line::from(match edit.later_context {
                    SelectiveLaterContext::Unobserved => {
                        "Later context: final state is unobserved."
                    }
                    SelectiveLaterContext::OpaqueSuffix => {
                        "Later context: unsupported code may change the outcome."
                    }
                    SelectiveLaterContext::KnownLaterTouch => {
                        "Later context: a later modeled operation touches this path spelling."
                    }
                    SelectiveLaterContext::PossibleAliasLaterTouch => {
                        "Later context: a later modeled operation may touch an alias."
                    }
                })
                .dim(),
            );
        }
        for barrier in &self.barriers {
            let reason = match &barrier.reason {
                SelectiveBarrierReason::Carrier { reason } => reason.as_str(),
                SelectiveBarrierReason::JavaScript { reason } => javascript::barrier_label(*reason),
                SelectiveBarrierReason::Python { reason } => match reason {
                    SelectivePythonBarrierReason::UnknownName => "unknown name",
                    SelectivePythonBarrierReason::UnmodeledCall => "unmodeled call",
                    SelectivePythonBarrierReason::UnmodeledImport => "unmodeled import",
                    SelectivePythonBarrierReason::UnsupportedOperation => "unsupported operation",
                    SelectivePythonBarrierReason::EscapingHandle => "escaping handle",
                },
            };
            lines.push(
                Line::from(format!(
                    "Unresolved region · {reason} · {}",
                    location(&barrier.evidence)
                ))
                .dim(),
            );
        }
        lines
    }
}

fn location(span: &SourceDescriptionSpan) -> String {
    format!(
        "source {}:{}:{}",
        span.source_id, span.start_line, span.start_column
    )
}

fn cwd_label(cwd: Option<&str>) -> String {
    cwd.map_or_else(|| "unavailable".to_owned(), |value| format!("{value:?}"))
}

fn target_label(target: &SelectiveTarget) -> String {
    format!(
        "{:?} · cwd {}",
        target.path,
        cwd_label(target.cwd.as_deref())
    )
}

fn encoding_label(encoding: SelectiveTextEncoding) -> &'static str {
    match encoding {
        SelectiveTextEncoding::Utf8 => "UTF-8",
        SelectiveTextEncoding::RuntimeDefault => "runtime default (assumed)",
    }
}

fn assumption_label(assumption: SelectivePythonAssumption) -> &'static str {
    match assumption {
        SelectivePythonAssumption::StandardPythonEvaluation => "standard Python evaluation",
        SelectivePythonAssumption::StandardBuiltinOpen => "standard builtin open",
        SelectivePythonAssumption::CanonicalPathlib => "canonical pathlib",
        SelectivePythonAssumption::PathlibNewlineSupport => {
            "pathlib newline API support (read_text: Python 3.13+; write_text: Python 3.10+)"
        }
        SelectivePythonAssumption::StandardTextMethods => "standard text methods",
        SelectivePythonAssumption::NoConcurrentMutation => "no concurrent mutation",
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod advanced_tests;
