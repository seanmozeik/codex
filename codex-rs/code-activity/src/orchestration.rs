//! Opt-in complete orchestration intent; no dispatch, policy or file observation.

mod input;
mod names;
mod strings;
mod syntax;
mod view;

pub use input::CompleteOrchestration;
pub use input::OrchestrationBuffer;
pub use input::OrchestrationInputError;
pub use input::OrchestratorBindings;
pub use view::OrchestrationActivity;
pub use view::OrchestrationChild;
pub use view::OrchestrationChildId;
pub use view::OrchestrationDisposition;
pub use view::OrchestrationFallback;
pub use view::OrchestrationGroup;
pub use view::OrchestrationJoin;

use crate::AnalysisLimits;
use crate::Analyzer;
use crate::CompleteScript;
use crate::Language;
use crate::PresentationDisposition;
use crate::Request;
use std::time::Duration;
use std::time::Instant;
use tree_sitter::ParseOptions;

#[derive(Clone, Copy)]
struct Deadline {
    started: Instant,
    limit: Duration,
}

impl Deadline {
    fn remaining(self) -> Result<Duration, OrchestrationFallback> {
        self.limit
            .checked_sub(self.started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(OrchestrationFallback::TimeBudget)
    }
}

impl Analyzer {
    /// Interprets a finite complete orchestration envelope under explicit trust.
    /// Every surrounding construct and every complete shell child must qualify;
    /// otherwise all groups and child effects are discarded. No snapshots or
    /// execution authority are accepted. Original source and identity survive.
    ///
    /// One cooperative deadline includes CST extraction and all child analyses.
    /// This method creates no threads and provides no hard response/CPU bound;
    /// hosts should submit the complete envelope as one bounded worker job.
    /// Promise settlement does not imply process completion or effect ordering.
    ///
    /// The finite grammar permits immutable literal/prior-literal command
    /// bindings and awaited direct `tools.exec_command({cmd: ...})` calls or
    /// native `Promise.all`/`allSettled` arrays of those calls. Other arguments,
    /// helpers, mutation, result inspection and template strings are unsupported.
    /// Single/double quoted strings support simple escapes, `\\xHH` and scalar
    /// `\\uHHHH`; surrogate and other escape forms fall back. Statements require
    /// separating semicolons. No inter-child filesystem order is inferred.
    ///
    /// Limits are eight children, eight bindings, eight join groups, 32 immediate
    /// named CST children per inspected node, and 16,320 decoded bytes per value.
    /// These are conservative support limits, not hidden partial-result cuts.
    #[must_use]
    pub fn analyze_orchestration(&mut self, input: CompleteOrchestration) -> OrchestrationActivity {
        let deadline = Deadline {
            started: Instant::now(),
            limit: input.limits().elapsed(),
        };
        let result = if input.environments_known() {
            self.orchestration_groups(&input, deadline)
        } else {
            Err(OrchestrationFallback::EnvironmentUnknown)
        };
        let (disposition, groups) = match result {
            Ok(groups) => (OrchestrationDisposition::Eligible, groups),
            Err(reason) => (OrchestrationDisposition::Fallback(reason), vec![]),
        };
        OrchestrationActivity {
            call_id: input.call_id,
            source: input.source,
            disposition,
            groups,
        }
    }

    fn orchestration_groups(
        &mut self,
        input: &CompleteOrchestration,
        deadline: Deadline,
    ) -> Result<Vec<OrchestrationGroup>, OrchestrationFallback> {
        deadline.remaining()?;
        let bytes = input.source.as_bytes();
        let mut progress = |_: &tree_sitter::ParseState| deadline.remaining().is_err();
        let tree = self.typescript.parse_with_options(
            &mut |offset, _| bytes.get(offset..).unwrap_or_default(),
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        );
        let Some(tree) = tree else {
            self.typescript.reset();
            return Err(if deadline.remaining().is_err() {
                OrchestrationFallback::TimeBudget
            } else {
                OrchestrationFallback::UnsupportedSource
            });
        };
        deadline.remaining()?;
        if tree.root_node().has_error() {
            return Err(OrchestrationFallback::UnsupportedSource);
        }
        let plan = syntax::extract(tree.root_node(), &input.source, deadline)?;
        let mut groups = Vec::new();
        for group in plan {
            let mut children = Vec::new();
            for child in group.children {
                let remaining = deadline.remaining()?;
                let request = Request::new(
                    format!("{}:child:{}", input.call_id.as_str(), child.id.index()).into(),
                    Language::Shell,
                    child.command.clone(),
                    None,
                );
                let script = CompleteScript::from_request(
                    request,
                    input.environment,
                    AnalysisLimits::new(input::MAX_SOURCE, remaining),
                )
                .map_err(|_| OrchestrationFallback::ResourceLimit)?;
                let activity = self.analyze_complete(script);
                deadline.remaining()?;
                if let PresentationDisposition::Fallback(reason) = activity.disposition() {
                    return Err(OrchestrationFallback::Child(reason));
                }
                children.push(OrchestrationChild {
                    id: child.id,
                    call_span: child.call_span,
                    command_span: child.command_span,
                    command: child.command,
                    activity,
                });
            }
            groups.push(OrchestrationGroup {
                join: group.join,
                span: group.span,
                children,
            });
        }
        deadline.remaining()?;
        Ok(groups)
    }
}
