use super::MAX_EXTENSION_EVENTS;
use super::MAX_REFERENCES;
use super::MAX_RETAINED_BYTES;
use super::Result;
use super::SourceDescriptionFallback;
use super::SourceReference;
use super::SourceReferenceBasis;
use super::SourceReferenceKind;
use super::State;
use crate::Span;

impl State<'_> {
    pub(super) fn reference(
        &mut self,
        kind: SourceReferenceKind,
        evidence: Span,
        literal: Option<String>,
    ) -> Result<()> {
        self.budget.check()?;
        if self.plan.references.len() >= MAX_REFERENCES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        let retained = literal
            .as_ref()
            .map_or(0, String::len)
            .checked_add(128)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        self.retain(retained)?;
        self.plan.references.push(SourceReference {
            basis: SourceReferenceBasis::SourceSyntax,
            kind,
            evidence,
            literal,
        });
        Ok(())
    }

    pub(super) fn event(&mut self) -> Result<()> {
        self.budget.check()?;
        self.plan.extension_events = self
            .plan
            .extension_events
            .checked_add(1)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if self.plan.extension_events > MAX_EXTENSION_EVENTS {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        self.check_retained()
    }

    pub(super) fn retain(&mut self, bytes: usize) -> Result<()> {
        self.retained = self
            .retained
            .checked_add(bytes)
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        self.check_retained()
    }

    pub(super) fn check_retained(&self) -> Result<()> {
        let charge = self
            .plan
            .extension_events
            .checked_mul(128)
            .and_then(|value| value.checked_add(self.plan.extension_name_bytes))
            .and_then(|value| value.checked_add(self.retained))
            .ok_or(SourceDescriptionFallback::SourceLimit)?;
        if charge > MAX_RETAINED_BYTES {
            return Err(SourceDescriptionFallback::SourceLimit);
        }
        Ok(())
    }
}
