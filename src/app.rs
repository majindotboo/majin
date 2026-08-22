use std::time::Duration;

use bevy::{app::ScheduleRunnerPlugin, prelude::*};
use bevy_ratatui::RatatuiPlugins;

use crate::{
    camera::CameraPlugin, harness::HarnessPlugin, persistence::PersistencePlugin, tui::TuiPlugin,
};

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
    Loading,
    Harness,
    Hydrate,
    Recover,
    Tui,
}

pub struct MajinPlugin;

impl Plugin for MajinPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Startup,
            (
                MajinStartupSet::Loading,
                MajinStartupSet::Harness,
                MajinStartupSet::Hydrate,
                MajinStartupSet::Recover,
                MajinStartupSet::Tui,
            )
                .chain(),
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
