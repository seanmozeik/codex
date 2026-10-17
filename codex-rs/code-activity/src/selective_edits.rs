//! Dependency-aware, conditional edit forecasts for pre-execution UI.
//!
//! This API is intentionally separate from complete source descriptions and
//! strict runtime-attested interpretation. It never reads files or executes
//! captured source, and it cannot change dispatch, approvals or sandbox policy.

mod dispatch;
mod input;
mod javascript;
mod model;
mod orchestration;
mod preview;
mod python;
mod shell;

pub use input::PythonWriteEndpoint;
pub use input::SelectiveEditInput;
pub use javascript::JavaScriptAssumption;
pub use javascript::JavaScriptBarrier;
pub use javascript::JavaScriptBarrierReason;
pub use javascript::JavaScriptDependency;
pub use javascript::JavaScriptDescriptorPosition;
pub use javascript::JavaScriptEdit;
pub use javascript::JavaScriptPlan;
pub use javascript::JavaScriptTextRecipe;
pub use javascript::JavaScriptWriteEffect;
pub use model::PredictedEdit;
pub use model::SelectiveBarrier;
pub use model::SelectiveCarrierAssumption;
pub use model::SelectiveCarrierBasis;
pub use model::SelectiveEditAnalysis;
pub use model::SelectiveEditBasis;
pub use model::SelectiveEditDetails;
pub use model::SelectiveEditDisposition;
pub use model::SelectiveEditHostProfile;
pub use model::SelectiveEditHostProfileAnalysis;
pub use model::SelectiveLaterContext;
pub use preview::ConditionalSnapshotPreview;
pub use preview::SnapshotByteRange;
pub use preview::SnapshotEncoding;
pub use preview::SnapshotEpoch;
pub use preview::SnapshotPreviewBasis;
pub use preview::SnapshotPreviewDetails;
pub use preview::SnapshotPreviewError;
pub use preview::SnapshotPreviewInput;
pub use preview::SnapshotPreviewLimit;
pub use preview::SnapshotPreviewLimits;
pub use preview::SnapshotPreviewScope;
pub use preview::SnapshotReadBinding;
pub use preview::SnapshotReadNewline;
pub use preview::SnapshotReplacement;
pub use preview::SnapshotTargetBinding;
pub use preview::SnapshotTextProfile;
pub use preview::SnapshotWriteNewline;
pub use preview::SourceReadId;
pub use preview::TrustedReadSnapshot;
pub use preview::TrustedTargetSnapshot;
pub use preview::materialize_snapshot_preview;
pub use python::*;

pub use preview::LogicalStreamBinding;
pub use preview::LogicalStreamPreviewInput;
pub use preview::LogicalStreamPreviewMetadata;
pub use preview::LogicalStreamRealization;
pub use preview::materialize_logical_stream_preview;
