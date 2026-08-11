mod common;
mod persistence_support;

use bevy::ecs::{message::Messages, system::Command};
use bevy::prelude::*;
use majin::*;
use pretty_assertions::assert_eq;
use rstest::rstest;
use std::{fs, time::Duration};

use persistence_support::*;

#[test]
fn commands_reject_before_harness_ready() {
    let mut app = App::new();
    app.insert_resource(PersistenceConfig::disabled());
    app.add_plugins(majin::MajinPlugin);
    let session = app.world_mut().spawn_empty().id();
    let mut cursor = app
        .world()
        .resource::<Messages<CommandResult>>()
        .get_cursor_current();
    SubmitPrompt {
        session,
        text: "blocked".into(),
    }
    .apply(app.world_mut());
    assert!(!app.world().contains_resource::<HarnessReady>());
    assert_eq!(
        cursor
            .read(app.world().resource::<Messages<CommandResult>>())
            .cloned()
            .collect::<Vec<_>>(),
        [CommandResult::PromptRejected {
            session,
            failure: CommandFailure::HarnessNotReady,
        }]
    );
}

#[rstest]
fn debounce_delays_first_snapshot_until_due() {
    let root = temp_path();
    let path = root.join("storage");
    let mut app = app(path.clone(), Duration::from_secs(60));
    assert!(app.world().contains_resource::<HarnessReady>());
    assert_eq!(
        app.world_mut()
            .query::<&Session>()
            .iter(app.world())
            .count(),
        1
    );
    assert!(!session_log(&path, SessionId(1)).exists());
    app.world_mut().resource_mut::<PersistenceConfig>().debounce = Duration::ZERO;
    app.update();
    assert!(session_log(&path, SessionId(1)).exists());
    fs::remove_dir_all(root).expect("remove temp state");
}
