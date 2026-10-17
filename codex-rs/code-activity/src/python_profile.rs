//! Explicit host assertions for the bounded Python presentation interpreter.

/// Additional runtime provenance asserted by the executor, never inferred from code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PythonRuntimeProfile {
    /// Ordinary strict analysis, retaining all module initialization uncertainty.
    #[default]
    Default,
    /// A controlled POSIX CPython child has preloaded canonical stdlib pathlib.
    ///
    /// The host asserts pinned interpreter/stdlib and bootstrap identities, fresh
    /// user globals, canonical cached `pathlib.Path`, and exact final source/cwd
    /// dispatch without intervening untrusted code. Flags alone do not prove this.
    /// Bootstrap imports have effects outside this user-source intent boundary.
    /// This profile supports only the documented bounded text/path grammar in
    /// direct Python; it is never inherited by shell or nested orchestration.
    PreloadedTrustedPathlib,
}
