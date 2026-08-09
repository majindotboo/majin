mod camera;
mod harness;
mod persistence;
mod tui;

use std::time::Duration;

use bevy::{app::ScheduleRunnerPlugin, prelude::*};
use bevy_ratatui::RatatuiPlugins;

pub use camera::CameraPlugin;
pub use harness::HarnessPlugin;
pub use persistence::PersistencePlugin;
pub use tui::{TranscriptItem, TranscriptKind, TuiPlugin, TuiView};

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

pub struct MajinPlugin;

impl Plugin for MajinPlugin {
    fn build(&self, app: &mut App) {
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
