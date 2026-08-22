mod camera;
mod harness;
mod persistence;
mod tui;

use std::time::Duration;

use bevy::{app::ScheduleRunnerPlugin, prelude::*};
use bevy_ratatui::RatatuiPlugins;

pub use camera::{
    CameraPlugin, ContextCamera, ContextDocument, ContextEntry, ProjectionError, TranscriptCamera,
    TranscriptRow, project_context, project_transcript,
};
pub use harness::{
    ActiveSession, Agent, AgentTool, AssistantMessage, BranchSelection, Compaction, HarnessPlugin,
    MessageId, Model, ModelChange, Provider, ProviderId, Recovery, SelectBranch, SelectSession,
    Sequence, Session, SessionId, SubmitPrompt, ToolDefinition, ToolId, Turn, TurnId, UserMessage,
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
