use serde::{Deserialize, Serialize};

use crate::{ModelApi, ModelStopReason, TurnFailure, WorkStatus};

pub(crate) const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Record {
    pub schema: u32,
    pub ordinal: u64,
    pub event: Event,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub(crate) enum Event {
    Session {
        id: u64,
        active_head: Option<u64>,
    },
    ContextCamera {
        budget: usize,
    },
    Turn {
        id: u64,
        parent: Option<u64>,
        sequence: u64,
        generation: u64,
    },
    UserMessage {
        id: u64,
        turn: u64,
        sequence: u64,
        text: String,
    },
    AssistantMessage {
        id: u64,
        turn: u64,
        sequence: u64,
        text: String,
    },
    ModelRequest {
        id: u64,
        turn: u64,
        model_id: String,
        provider_id: u64,
        generation: u64,
        previous_tool_use: Option<u64>,
        status: StoredWorkStatus,
        sequence: u64,
    },
    ModelResponse {
        request: u64,
        turn: u64,
        model_id: String,
        provider_id: u64,
        generation: u64,
        response_id: String,
        api: StoredModelApi,
        input_tokens: u32,
        output_tokens: u32,
        stop_reason: StoredModelStopReason,
        opaque_replay: String,
        sequence: u64,
    },
    ToolUse {
        id: u64,
        turn: u64,
        tool_id: u64,
        model_id: String,
        provider_id: u64,
        generation: u64,
        input: String,
        status: StoredWorkStatus,
        sequence: u64,
    },
    ToolOutcome {
        tool_use: u64,
        tool_call_id: u64,
        turn: u64,
        generation: u64,
        output: String,
        sequence: u64,
    },
    TurnCompleted {
        turn: u64,
        generation: u64,
        sequence: u64,
    },
    TurnCancelled {
        turn: u64,
        generation: u64,
        sequence: u64,
    },
    TurnInterrupted {
        turn: u64,
        generation: u64,
        sequence: u64,
    },
    TurnFailed {
        turn: u64,
        generation: u64,
        failure: StoredTurnFailure,
        sequence: u64,
    },
    BranchSelection {
        head: u64,
        sequence: u64,
    },
    ModelChange {
        turn: u64,
        model_id: String,
        provider_id: u64,
        sequence: u64,
    },
    Compaction {
        turn: u64,
        summary: String,
        sequence: u64,
    },
    Recovery {
        turn: u64,
        text: String,
        sequence: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum StoredWorkStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl From<WorkStatus> for StoredWorkStatus {
    fn from(value: WorkStatus) -> Self {
        match value {
            WorkStatus::Pending => Self::Pending,
            WorkStatus::Running => Self::Running,
            WorkStatus::Succeeded => Self::Succeeded,
            WorkStatus::Failed => Self::Failed,
            WorkStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<StoredWorkStatus> for WorkStatus {
    fn from(value: StoredWorkStatus) -> Self {
        match value {
            StoredWorkStatus::Pending => Self::Pending,
            StoredWorkStatus::Running => Self::Running,
            StoredWorkStatus::Succeeded => Self::Succeeded,
            StoredWorkStatus::Failed => Self::Failed,
            StoredWorkStatus::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum StoredModelApi {
    Fake,
}

impl From<ModelApi> for StoredModelApi {
    fn from(value: ModelApi) -> Self {
        match value {
            ModelApi::Fake => Self::Fake,
        }
    }
}

impl From<StoredModelApi> for ModelApi {
    fn from(value: StoredModelApi) -> Self {
        match value {
            StoredModelApi::Fake => Self::Fake,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum StoredModelStopReason {
    ToolUse,
    Complete,
}

impl From<ModelStopReason> for StoredModelStopReason {
    fn from(value: ModelStopReason) -> Self {
        match value {
            ModelStopReason::ToolUse => Self::ToolUse,
            ModelStopReason::Complete => Self::Complete,
        }
    }
}

impl From<StoredModelStopReason> for ModelStopReason {
    fn from(value: StoredModelStopReason) -> Self {
        match value {
            StoredModelStopReason::ToolUse => Self::ToolUse,
            StoredModelStopReason::Complete => Self::Complete,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredTurnFailure {
    pub kind: StoredTurnFailureKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum StoredTurnFailureKind {
    Provider,
    Tool,
    Recovery,
}

impl From<&TurnFailure> for StoredTurnFailure {
    fn from(value: &TurnFailure) -> Self {
        let (kind, message) = match value {
            TurnFailure::Provider(failure) => (StoredTurnFailureKind::Provider, &failure.message),
            TurnFailure::Tool(failure) => (StoredTurnFailureKind::Tool, &failure.message),
            TurnFailure::Recovery(failure) => (StoredTurnFailureKind::Recovery, &failure.message),
        };
        Self {
            kind,
            message: message.clone(),
        }
    }
}

impl Event {
    pub(crate) fn key(&self) -> String {
        match self {
            Self::Session { .. } => "session".into(),
            Self::ContextCamera { .. } => "context-camera".into(),
            Self::Turn { id, .. } => format!("turn:{id}"),
            Self::UserMessage { id, .. } => format!("user-message:{id}"),
            Self::AssistantMessage { id, .. } => format!("assistant-message:{id}"),
            Self::ModelRequest { id, .. } => format!("model-request:{id}"),
            Self::ModelResponse { response_id, .. } => format!("model-response:{response_id}"),
            Self::ToolUse { id, .. } => format!("tool-use:{id}"),
            Self::ToolOutcome { tool_call_id, .. } => format!("tool-outcome:{tool_call_id}"),
            Self::TurnCompleted { turn, .. } => format!("turn-completed:{turn}"),
            Self::TurnCancelled { turn, .. } => format!("turn-cancelled:{turn}"),
            Self::TurnInterrupted { turn, .. } => format!("turn-interrupted:{turn}"),
            Self::TurnFailed { turn, .. } => format!("turn-failed:{turn}"),
            Self::BranchSelection { sequence, .. } => format!("branch-selection:{sequence}"),
            Self::ModelChange { sequence, .. } => format!("model-change:{sequence}"),
            Self::Compaction { sequence, .. } => format!("compaction:{sequence}"),
            Self::Recovery { sequence, .. } => format!("recovery:{sequence}"),
        }
    }

    pub(crate) fn sequence(&self) -> Option<u64> {
        match self {
            Self::Session { .. } | Self::ContextCamera { .. } => None,
            Self::Turn { sequence, .. }
            | Self::UserMessage { sequence, .. }
            | Self::AssistantMessage { sequence, .. }
            | Self::ModelRequest { sequence, .. }
            | Self::ModelResponse { sequence, .. }
            | Self::ToolUse { sequence, .. }
            | Self::ToolOutcome { sequence, .. }
            | Self::TurnCompleted { sequence, .. }
            | Self::TurnCancelled { sequence, .. }
            | Self::TurnInterrupted { sequence, .. }
            | Self::TurnFailed { sequence, .. }
            | Self::BranchSelection { sequence, .. }
            | Self::ModelChange { sequence, .. }
            | Self::Compaction { sequence, .. }
            | Self::Recovery { sequence, .. } => Some(*sequence),
        }
    }
}
