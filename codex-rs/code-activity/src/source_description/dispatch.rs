//! One atomic collector and elapsed budget across every decoded child source.

use super::CompleteSourceDescriptionInput;
use super::DescriptionBudget;
use super::MAX_REFERENCES;
use super::MAX_RETAINED_BYTES;
use super::PatchId;
use super::SourceDescription;
use super::SourceDescriptionFallback;
use super::SourcePatch;
use super::SourceReference;
use super::javascript;
use super::orchestration;
use super::parse;
use super::patch;
use super::python;
use super::shell;
use crate::Analyzer;
use crate::Language;
use crate::Source;
use crate::SourceId;
use crate::Span;

const MAX_SOURCES: usize = 32;
const MAX_CARRIER_DEPTH: usize = 8;
const MAX_EXTENSION_EVENTS: usize = 4096;

#[derive(Default)]
struct Collection {
    sources: Vec<Source>,
    source_texts: Vec<String>,
    references: Vec<SourceReference>,
    patches: Vec<SourcePatch>,
    patch_counts: patch::Counts,
    decoded_bytes: usize,
    retained_bytes: usize,
    extension_events: usize,
}

impl Analyzer {
    /// Describes an entire source using a closed syntax-only grammar.
    /// Unknown runtime bindings are not replaced with trust assumptions. Label
    /// this result "Source references", never effects or file changes. Unsupported
    /// syntax anywhere, including decoded children, discards the whole result.
    /// All children consume one elapsed budget and aggregate allocation bounds.
    #[must_use]
    pub fn describe_complete(
        &mut self,
        input: CompleteSourceDescriptionInput,
    ) -> SourceDescription {
        let budget = DescriptionBudget::new(input.limits.elapsed());
        let mut collection = Collection::default();
        let result = self.describe_source(
            input.language,
            &input.source,
            None,
            0,
            budget,
            &mut collection,
        );
        match result.and_then(|()| budget.check()) {
            Ok(()) => SourceDescription::complete(
                input.call_id,
                collection.sources,
                collection.source_texts,
                collection.references,
                collection.patches,
            ),
            Err(reason) => SourceDescription::fallback(input.call_id, reason),
        }
    }

    fn describe_source(
        &mut self,
        language: Language,
        source: &str,
        parent: Option<Span>,
        depth: usize,
        budget: DescriptionBudget,
        collection: &mut Collection,
    ) -> Result<(), SourceDescriptionFallback> {
        budget.check()?;
        if depth > MAX_CARRIER_DEPTH {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        let id = collection.source(language, source, parent)?;
        match language {
            Language::Python => {
                let references = python::describe(&mut self.python, source, budget)?;
                collection.references(id, references)?;
            }
            Language::Shell => {
                let tree = parse(&mut self.preview_shell, source, budget)?;
                let plan = shell::extract(tree.root_node(), source, budget)?;
                collection.extension(plan.extension_events, plan.extension_name_bytes)?;
                collection.references(id, plan.references)?;
                for child in plan.children {
                    let mut evidence = child.evidence;
                    evidence.source_id = id;
                    self.describe_source(
                        child.language,
                        &child.source,
                        Some(evidence),
                        depth + 1,
                        budget,
                        collection,
                    )?;
                }
            }
            Language::TypeScript => {
                let tree = parse(&mut self.typescript, source, budget)?;
                let plan = match orchestration::extract(tree.root_node(), source, budget) {
                    Ok(plan) => plan,
                    Err(SourceDescriptionFallback::UnsupportedSource) => {
                        let references = javascript::extract(tree.root_node(), source, budget)?;
                        collection.references(id, references)?;
                        return budget.check();
                    }
                    Err(reason) => return Err(reason),
                };
                collection.extension(plan.extension_events, plan.extension_name_bytes)?;
                collection.references(id, plan.references)?;
                for invocation in plan.invocations {
                    match invocation {
                        orchestration::Invocation::Command {
                            command,
                            mut evidence,
                        } => {
                            evidence.source_id = id;
                            self.describe_source(
                                Language::Shell,
                                &command,
                                Some(evidence),
                                depth + 1,
                                budget,
                                collection,
                            )?;
                        }
                        orchestration::Invocation::Patch(invocation) => {
                            collection.patch(id, invocation, budget)?;
                        }
                        orchestration::Invocation::Poll(_) => {}
                    }
                }
            }
        }
        budget.check()
    }
}

impl Collection {
    // Reader-local admission cannot reset the charge shared by all owned sources.
    fn extension(
        &mut self,
        events: usize,
        name_bytes: usize,
    ) -> Result<(), SourceDescriptionFallback> {
        let limit = SourceDescriptionFallback::SourceLimit;
        self.extension_events = self.extension_events.checked_add(events).ok_or(limit)?;
        if self.extension_events > MAX_EXTENSION_EVENTS {
            return Err(limit);
        }
        let retained = events
            .checked_mul(128)
            .and_then(|bytes| bytes.checked_add(name_bytes))
            .ok_or(limit)?;
        self.retain(retained)
    }

    fn source(
        &mut self,
        language: Language,
        source: &str,
        parent: Option<Span>,
    ) -> Result<SourceId, SourceDescriptionFallback> {
        self.decoded(source.len())?;
        if self.sources.len() >= MAX_SOURCES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        let id = SourceId(self.sources.len());
        self.sources.push(Source {
            id,
            language,
            parent,
        });
        self.source_texts.push(source.to_owned());
        Ok(id)
    }

    fn decoded(&mut self, bytes: usize) -> Result<(), SourceDescriptionFallback> {
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(bytes)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if self.decoded_bytes > crate::GUARD_MAX_SOURCE_BYTES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        Ok(())
    }

    fn patch(
        &mut self,
        source_id: SourceId,
        mut invocation: patch::LiteralPatchInvocation,
        budget: DescriptionBudget,
    ) -> Result<(), SourceDescriptionFallback> {
        budget.check()?;
        if self.patches.len() >= patch::MAX_PATCHES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        self.decoded(invocation.payload.len())?;
        invocation.invocation.source_id = source_id;
        invocation.argument.source_id = source_id;
        let scanned = patch::scan(invocation, PatchId(self.patches.len()), budget)?;
        self.patch_counts.add(&scanned.counts)?;
        self.retain(scanned.retained_bytes()?)?;
        // Reserve the complete parent charge before the original parser allocates.
        self.patches.push(scanned.validate(budget)?);
        Ok(())
    }

    fn retain(&mut self, bytes: usize) -> Result<(), SourceDescriptionFallback> {
        self.retained_bytes = self
            .retained_bytes
            .checked_add(bytes)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if self.retained_bytes > MAX_RETAINED_BYTES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        Ok(())
    }

    fn references(
        &mut self,
        id: SourceId,
        references: Vec<SourceReference>,
    ) -> Result<(), SourceDescriptionFallback> {
        if references.len() > MAX_REFERENCES.saturating_sub(self.references.len()) {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        for mut reference in references {
            self.retain(
                reference
                    .literal
                    .as_ref()
                    .map_or(128, |value| value.len() + 128),
            )?;
            reference.evidence.source_id = id;
            self.references.push(reference);
        }
        Ok(())
    }
}

#[cfg(test)]
mod accounting_tests;
