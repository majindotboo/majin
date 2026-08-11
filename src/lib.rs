mod app;
mod camera;
mod execution;
mod harness;
mod persistence;
mod tui;

pub(crate) use app::MajinStartupSet;
pub use app::{MajinPlugin, MajinSet, run};
pub(crate) use camera::project_context_for;

pub use camera::{
    CameraPlugin, ContextCamera, ContextDocument, ContextEntry, PersistentContextCamera,
    ProjectionError, TranscriptCamera, TranscriptRow, TranscriptWork, project_context,
    project_transcript,
};
pub use harness::{
    ActiveAgent, ActiveSession, Agent, AgentTool, AssistantMessage, BranchSelection,
    CommandFailure, CommandResult, Compaction, HarnessPlugin, HarnessReady, InterruptTurn,
    MessageId, Model, ModelApi, ModelChange, ModelOutput, ModelReply, ModelRequest, ModelRequestId,
    ModelResponse, ModelResult, ModelStopReason, ModelUsage, PersistenceFailure, Provider,
    ProviderFailure, ProviderId, Recovery, RecoveryFailure, SelectBranch, SelectSession, Sequence,
    Session, SessionId, SubmitPrompt, ToolCallId, ToolDefinition, ToolFailure, ToolId, ToolOutcome,
    ToolResult, ToolUse, Turn, TurnCancelled, TurnCompleted, TurnFailed, TurnFailure, TurnId,
    TurnInterrupted, UserMessage, WorkStatus,
};
pub use persistence::{PersistenceConfig, PersistencePlugin};
pub use tui::{TerminalTranscriptViewport, TuiFocus, TuiPlugin, TuiView};
