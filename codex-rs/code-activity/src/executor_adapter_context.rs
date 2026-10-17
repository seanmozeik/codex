//! Executor-owned source and context assertions; none are parser attestations.

use crate::Language;
use crate::OrchestratorBindings;

/// Runtime contract asserted by the host for one finalized execution attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExecutorRuntimeProfile {
    /// Persistent state or runtime/module identity has not been established.
    Unknown,
    /// All ordinary [`crate::EnvironmentAssumptions::FreshProcess`] obligations.
    OrdinaryFresh,
    /// A ready, isolated direct Python child with canonical pathlib preloaded.
    /// The host pins the runtime, installation and bootstrap and verifies its
    /// readiness before analysis. Bootstrap effects are outside user-source scope.
    /// This profile never extends to shell children or orchestration envelopes.
    PreloadedPathlib,
}

/// Host-owned provenance for the same immutable launch passed to dispatch.
///
/// This is an assertion, not verification: no constructor checks the runtime,
/// module installation, bootstrap, handshake, permissions or filesystem. Do not
/// derive it from model-supplied JSON, source spelling or isolated-startup flags.
/// A host must invalidate it after source, cwd, environment, wrapper or attempt
/// changes. Interior mutation and executable-resolution races remain host duties.
/// No environment values are copied, inspected, logged or serialized here.
#[derive(Debug)]
#[non_exhaustive]
pub struct ExecutorContext<'a> {
    launch_id: &'a str,
    readiness_identity: &'a str,
    profile: ExecutorRuntimeProfile,
    bindings: OrchestratorBindings,
}

impl<'a> ExecutorContext<'a> {
    /// Records the host's explicit assertion for an already finalized attempt.
    /// For `PreloadedPathlib`, verify the waiting child's bootstrap readiness
    /// first; analysis precedes release of user source, not process startup.
    #[must_use]
    pub const fn host_asserted(
        launch_id: &'a str,
        readiness_identity: &'a str,
        profile: ExecutorRuntimeProfile,
        bindings: OrchestratorBindings,
    ) -> Self {
        Self {
            launch_id,
            readiness_identity,
            profile,
            bindings,
        }
    }

    /// Exact opaque identity of the finalized launch/environment configuration.
    #[must_use]
    pub const fn launch_id(&self) -> &'a str {
        self.launch_id
    }

    /// Exact opaque readiness evidence supplied by the host, never parser proof.
    #[must_use]
    pub const fn readiness_identity(&self) -> &'a str {
        self.readiness_identity
    }

    /// Runtime assumption selected explicitly by the host.
    #[must_use]
    pub const fn profile(&self) -> ExecutorRuntimeProfile {
        self.profile
    }

    /// Outer native binding assumption, independent of child runtime identity.
    #[must_use]
    pub const fn bindings(&self) -> OrchestratorBindings {
        self.bindings
    }
}

/// Complete source view derived by the host from its finalized launch object.
/// The host assembles all transport fragments and applies relevant hooks first.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum ExecutorSource<'a> {
    /// Exact script bytes and the executor-established language/carrier semantics.
    Script {
        /// Language established by the executor, not guessed from source text.
        language: Language,
        /// Complete decoded source, unchanged through dispatch.
        source: &'a str,
    },
    /// Complete outer source; extracted groups remain provisional before child hooks.
    Orchestration {
        /// Complete decoded JavaScript envelope.
        source: &'a str,
    },
}

impl<'a> ExecutorSource<'a> {
    /// Exact complete decoded source; this adapter never rewrites it.
    #[must_use]
    pub const fn source(self) -> &'a str {
        match self {
            Self::Script { source, .. } | Self::Orchestration { source } => source,
        }
    }
}

/// Borrowed immutable launch and its complete source/context view.
///
/// The host must derive this view from `original`; the adapter cannot prove that
/// arbitrary metadata describes an opaque launch object. Keep argv, environment,
/// cwd and authorization state in that object, without interior mutation until
/// dispatch. This type alone cannot dispatch: call [`Self::prepare`] first.
///
/// ```compile_fail
/// use codex_code_activity::FinalExecutorInput;
/// fn premature(input: FinalExecutorInput<'_, ()>) {
///     input.dispatch(|_| ());
/// }
/// ```
#[non_exhaustive]
pub struct FinalExecutorInput<'a, L> {
    original: &'a L,
    call_id: &'a str,
    source: ExecutorSource<'a>,
    cwd: Option<&'a str>,
    context: &'a ExecutorContext<'a>,
}

impl<L> std::fmt::Debug for FinalExecutorInput<'_, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FinalExecutorInput").finish_non_exhaustive()
    }
}

impl<'a, L> FinalExecutorInput<'a, L> {
    /// Borrows complete finalized input without allocating or analyzing it.
    /// No permission or environment verification is performed. `cwd` is literal
    /// executor-local spelling, never a conversion from a remote cwd URI.
    #[must_use]
    pub const fn new(
        original: &'a L,
        call_id: &'a str,
        source: ExecutorSource<'a>,
        cwd: Option<&'a str>,
        context: &'a ExecutorContext<'a>,
    ) -> Self {
        Self {
            original,
            call_id,
            source,
            cwd,
            context,
        }
    }

    /// Same executor-owned launch object on every presentation outcome.
    #[must_use]
    pub const fn original(&self) -> &'a L {
        self.original
    }

    /// Host call identity, distinct from source-level orchestration ordinals.
    #[must_use]
    pub const fn call_id(&self) -> &'a str {
        self.call_id
    }

    /// Exact original complete source view.
    #[must_use]
    pub const fn source(&self) -> ExecutorSource<'a> {
        self.source
    }

    /// Exact borrowed executor cwd; no normalization or filesystem lookup occurs.
    #[must_use]
    pub const fn cwd(&self) -> Option<&'a str> {
        self.cwd
    }

    /// Same executor-owned context and profile used for preparation and dispatch.
    #[must_use]
    pub const fn context(&self) -> &'a ExecutorContext<'a> {
        self.context
    }
}
