//! Wire markers with one valid value for each lifecycle phase.

use serde::Serialize;

/// The version of the activity wire format.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportVersion {
    /// The original activity report format.
    #[serde(rename = "codeActivity/v1")]
    V1,
}

/// Marker available only on provisional source reports.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum PendingPhase {
    /// The model is still producing the tool input.
    #[serde(rename = "pending")]
    Pending,
}

/// Marker for a report whose original files have been captured.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum PreparedPhase {
    /// The final call has a baseline and can be dispatched by the host.
    #[serde(rename = "prepared")]
    Prepared,
}

/// Marker for a report joined with a terminal tool outcome.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum CompletedPhase {
    /// The host supplied a terminal result and endpoint observations.
    #[serde(rename = "completed")]
    Completed,
}

/// What file comparisons establish about their provenance.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObservationBasis {
    /// Endpoints from one observation interval, without proof of authorship.
    #[serde(rename = "captureInterval")]
    CaptureInterval,
}

/// Recognised transformations of content read from a target.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum Transform {
    /// Whitespace or case normalization; final bytes require observation.
    TextNormalization,
    /// Literal or regular-expression replacement through a string method.
    TextReplacement,
    /// A Python regular-expression substitution.
    RegexSubstitution,
    /// A slice or substring of existing content.
    TextSlice,
    /// Concatenation that retains the same content origin.
    TextConcatenation,
}
