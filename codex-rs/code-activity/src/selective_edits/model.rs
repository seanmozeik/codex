//! Conditional forecasts are separate from source references and observed diffs.

use super::JavaScriptAssumption;
use super::JavaScriptBarrier;
use super::JavaScriptEdit;
use super::PythonAssumption;
use super::PythonBarrier;
use super::PythonEdit;
use crate::CallId;
use crate::Language;
use crate::Source;
use crate::SourceDescriptionFallback;
use crate::SourcePatch;
use crate::Span;
use serde::Deserialize;
use serde::Serialize;

/// Evidence supporting a conditional edit forecast, never execution authority.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectiveEditBasis {
    /// A closed recipe under the serialized standard-API assumptions.
    StandardApiForecast,
    /// Exact literal arguments for an unshadowed expected host-tool binding.
    HostToolRequest,
}

/// What analysis knows about the source following a retained operation.
/// No variant establishes the final state of the filesystem.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectiveLaterContext {
    /// No additional opaque suffix was encountered; execution is unobserved.
    Unobserved,
    /// Later unsupported code may change the filesystem or execution outcome.
    OpaqueSuffix,
    /// A later modeled operation targets the same path spelling and cwd anchor.
    /// This can include a target-only suffix summary without a byte forecast.
    /// Reaching that operation remains conditional; independently retained
    /// barriers and language-specific final-state opacity still apply.
    KnownLaterTouch,
    /// A later modeled operation may touch an alias of this target.
    PossibleAliasLaterTouch,
}

/// An explicit standard command/interpreter assumption for an owned child.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectiveCarrierBasis {
    /// Command lookup, aliases and startup files preserve the modeled bindings,
    /// cwd and environment. This includes default login shells and `-lc`; the
    /// parser does not inspect or attest startup/profile execution.
    StandardShellDispatchAndStartup,
    /// The selected command follows the Python 3 command-line and evaluation
    /// rules modeled here. Unversioned `python` does not establish this identity.
    StandardPython3Cli,
    /// The selected node/nodejs command follows ordinary Node CommonJS CLI
    /// rules. Startup hooks, module identity and environment are assumptions.
    StandardNodeCommonJsCli,
}

/// An explicit standard command/interpreter assumption for an owned child.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectiveCarrierAssumption {
    /// Exact conditional dispatch/startup assumption, never inferred from source.
    pub basis: SelectiveCarrierBasis,
    /// Grammar selected by the source's command spelling.
    pub language: Language,
    /// The call or command whose ordinary dispatch is being assumed.
    pub invocation: Span,
}

/// Exact language-specific operation data; no filesystem observations appear here.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SelectiveEditDetails {
    /// A Node CommonJS text write with frozen source dependencies.
    JavaScript {
        /// One conditional operation from the bounded Node producer.
        prediction: JavaScriptEdit,
        /// Named hypotheses; no runtime identity is attested by source.
        assumptions: Vec<JavaScriptAssumption>,
    },
    /// A Python operation with frozen target/content dependencies.
    Python {
        /// One conditional operation from the bounded Python interpreter.
        prediction: PythonEdit,
        /// Named assumptions, not claims about a captured process's identity.
        assumptions: Vec<PythonAssumption>,
    },
    /// Exact submitted patch text and its parsed file directives.
    Patch {
        /// Requested patch, never an applied or observed diff.
        patch: SourcePatch,
        /// Host-provided path anchor, if available; never inferred from disk.
        cwd: Option<String>,
    },
}

/// A retained operation in source evaluation order.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictedEdit {
    /// Zero-based order across retained outer and owned-child operations.
    pub ordinal: usize,
    /// Conditional evidence basis.
    pub basis: SelectiveEditBasis,
    /// Exact source sink or host-tool invocation.
    pub evidence: Span,
    /// Frozen target, recipe or literal patch request.
    pub details: SelectiveEditDetails,
    /// Modeled earlier invocations that must complete normally to reach this edit.
    /// Arbitrary opaque predecessors are never rescued by this field.
    pub predecessor_conditions: Vec<Span>,
    /// Explicit assumptions for shell/interpreter carriers containing this edit.
    pub carrier_assumptions: Vec<SelectiveCarrierAssumption>,
    /// Separate uncertainty about the suffix and final disk state.
    pub later_context: SelectiveLaterContext,
}

/// A source region whose effects prevent further dependent forecasts.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectiveBarrier {
    /// Region establishing the uncertainty, not an executed trace.
    pub evidence: Span,
    /// Stable diagnostic category from the bounded carrier interpreter.
    pub reason: String,
}

/// Availability of the new conditional analysis, independent of the old API.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectiveEditDisposition {
    /// The bounded slice completed; barriers may still omit individual edits.
    Analyzed,
    /// No forecasts were published from an incomplete or invalid source analysis.
    Unavailable(SourceDescriptionFallback),
}

/// Conditional pre-execution findings with no observed effects or disk diffs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectiveEditAnalysis {
    pub(crate) call_id: CallId,
    pub(crate) disposition: SelectiveEditDisposition,
    pub(crate) sources: Vec<Source>,
    #[serde(skip)]
    pub(crate) source_texts: Vec<String>,
    pub(crate) edits: Vec<PredictedEdit>,
    pub(crate) barriers: Vec<SelectiveBarrier>,
    pub(crate) python_barriers: Vec<PythonBarrier>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) javascript_barriers: Vec<JavaScriptBarrier>,
}

impl SelectiveEditAnalysis {
    /// Availability; an analyzed result can contain omissions and zero edits.
    #[must_use]
    pub const fn disposition(&self) -> SelectiveEditDisposition {
        self.disposition
    }

    /// Ordered conditional operations, never a guaranteed final filesystem state.
    #[must_use]
    pub fn edits(&self) -> &[PredictedEdit] {
        &self.edits
    }

    /// Source provenance for all published evidence.
    #[must_use]
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// Exact in-memory source texts for independent local evidence checks.
    /// Source texts are excluded from the serialized analysis.
    #[must_use]
    pub fn source_texts(&self) -> &[String] {
        &self.source_texts
    }

    /// Carrier-level dependency barriers.
    #[must_use]
    pub fn barriers(&self) -> &[SelectiveBarrier] {
        &self.barriers
    }

    /// Python-local dependency barriers, retaining their exact categories.
    #[must_use]
    pub fn python_barriers(&self) -> &[PythonBarrier] {
        &self.python_barriers
    }

    /// Node CommonJS dependency barriers, retaining their exact categories.
    #[must_use]
    pub fn javascript_barriers(&self) -> &[JavaScriptBarrier] {
        &self.javascript_barriers
    }

    /// Host-supplied call identity, used to bind a supplied snapshot preview.
    #[must_use]
    pub fn call_id(&self) -> &CallId {
        &self.call_id
    }

    pub(crate) fn empty(call_id: CallId, disposition: SelectiveEditDisposition) -> Self {
        Self {
            call_id,
            disposition,
            sources: Vec::new(),
            source_texts: Vec::new(),
            edits: Vec::new(),
            barriers: Vec::new(),
            python_barriers: Vec::new(),
            javascript_barriers: Vec::new(),
        }
    }
}

/// Caller-selected host premises, not verified runtime identity.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectiveEditHostProfile {
    /// Whole-cell entry in the fresh built-in CodeMode host: pristine standard
    /// prototypes, exact built-in patch binding and JSON result conversion,
    /// and no foreign or interleaving user callbacks. Observe/resume is not
    /// a fresh entry. Tool and display normal completion remain conditions.
    FreshBuiltinCodeModePatchDisplayV1,
}

/// A forecast qualified by additional caller-supplied host premises.
///
/// This distinct envelope deliberately provides no unlabelled analysis getter
/// or conversion into the ordinary result used by existing UI consumers.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectiveEditHostProfileAnalysis {
    pub(crate) host_profile: SelectiveEditHostProfile,
    pub(crate) analysis: SelectiveEditAnalysis,
}

impl SelectiveEditHostProfileAnalysis {
    /// Exact in-memory sources for checking evidence in the labelled envelope.
    #[must_use]
    pub fn source_texts(&self) -> &[String] {
        self.analysis.source_texts()
    }
}
