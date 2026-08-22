mod camera;
mod execution;
mod harness;
mod persistence;
mod tui;

pub(crate) use app::MajinStartupSet;
pub use app::{MajinPlugin, MajinSet, run};
pub(crate) use camera::project_context_for;

pub use camera::{
    CameraPlugin, ContextCamera, ContextDocument, ContextEntry, ProjectionError, TranscriptCamera,
    TranscriptRow, project_context, project_transcript,
};
pub use harness::{
    ActiveAgent, ActiveSession, Agent, AgentTool, AssistantMessage, BranchSelection,
    CommandFailure, CommandResult, Compaction, HarnessPlugin, InterruptTurn, MessageId, Model,
    ModelApi, ModelChange, ModelOutput, ModelReply, ModelRequest, ModelRequestId, ModelResponse,
    ModelResult, ModelStopReason, ModelUsage, Provider, ProviderFailure, ProviderId, Recovery,
    RecoveryFailure, SelectBranch, SelectSession, Sequence, Session, SessionId, SubmitPrompt,
    ToolCallId, ToolDefinition, ToolFailure, ToolId, ToolOutcome, ToolResult, ToolUse, Turn,
    TurnCancelled, TurnCompleted, TurnFailed, TurnFailure, TurnId, UserMessage, WorkStatus,
};
pub use persistence::PersistencePlugin;
pub use tui::{TerminalTranscriptViewport, TuiPlugin, TuiView};

pub fn run() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f32(
                1. / 60.,
            ))),
            RatatuiPlugins {
                enable_mouse_capture: true,
                ..default()
            },
            MajinPlugin,
        ))
        .run();
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MajinSet {
    Input,
    Orchestrate,
    Dispatch,
    Apply,
    Project,
    Persist,
    Render,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MajinStartupSet {
    Harness,
    Tui,
}

pub struct MajinPlugin;

impl Plugin for MajinPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Startup,
            (MajinStartupSet::Harness, MajinStartupSet::Tui).chain(),
        );
        app.configure_sets(
            Update,
            (
                MajinSet::Input,
                MajinSet::Orchestrate,
                MajinSet::Dispatch,
                MajinSet::Apply,
                MajinSet::Project,
                MajinSet::Persist,
                MajinSet::Render,
            )
                .chain(),
        )
        .add_plugins((HarnessPlugin, CameraPlugin, PersistencePlugin, TuiPlugin));
    }
}
