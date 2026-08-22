use assert_fs::TempDir;
use bevy::ecs::{message::Messages, system::Command};
use bevy::prelude::*;
use majin::*;
use pretty_assertions::assert_eq;
use rstest::rstest;
use std::{fs, time::Duration};

use crate::common::unstarted_app;
use crate::persistence_support::*;

#[rstest]
fn recovery_cancels_orphaned_model_and_tool_work_once_per_turn(temp_dir: TempDir) {
    let path = temp_dir.path().join("storage");
    let mut first = app(path.clone(), Duration::ZERO);
    let session = first.world().resource::<ActiveSession>().0;
    let agent = first.world().resource::<ActiveAgent>().0;
    let model = first.world().get::<Agent>(agent).unwrap().model;
    let provider = first.world().get::<Model>(model).unwrap().provider;
    let tool = single::<ToolDefinition>(&mut first);
    let model_turn = first
        .world_mut()
        .spawn(Turn {
            id: TurnId(1),
            session,
            parent: None,
            sequence: majin::Sequence(1),
            generation: 4,
        })
        .id();
    let tool_turn = first
        .world_mut()
        .spawn(Turn {
            id: TurnId(2),
            session,
            parent: None,
            sequence: majin::Sequence(2),
            generation: 7,
        })
        .id();
    first.world_mut().spawn(ModelRequest {
        id: ModelRequestId(1),
        turn: model_turn,
        agent,
        model,
        provider,
        generation: 4,
        previous_tool_use: None,
        status: WorkStatus::Pending,
        sequence: majin::Sequence(3),
    });
    first.world_mut().spawn(ToolUse {
        id: ToolCallId(1),
        turn: tool_turn,
        agent,
        tool,
        model,
        provider,
        generation: 7,
        input: "recover".into(),
        status: WorkStatus::Pending,
        sequence: majin::Sequence(4),
    });
    let view = single::<TuiView>(&mut first);
    first.world_mut().get_mut::<TuiView>(view).unwrap().composer = "excluded composer".into();
    first.update();
    assert!(
        first
            .world_mut()
            .query::<&ModelRequest>()
            .iter(first.world())
            .any(|request| request.status == WorkStatus::Running)
    );
    let snapshot = fs::read_to_string(session_log(&path, SessionId(1))).expect("running work log");
    for excluded in [
        "TuiView",
        "TranscriptCamera",
        "TerminalTranscriptViewport",
        "ModelTask",
        "ToolTask",
        "ProviderExecutor",
        "ToolExecutor",
        "ActiveSession",
        "ActiveAgent",
        "ContextDocument",
        "excluded composer",
    ] {
        assert!(!snapshot.contains(excluded), "snapshot includes {excluded}");
    }
    drop(first);

    let mut restored = app(path.clone(), Duration::ZERO);
    assert_eq!(
        restored
            .world_mut()
            .query::<&TurnInterrupted>()
            .iter(restored.world())
            .count(),
        2
    );
    assert_eq!(
        restored
            .world_mut()
            .query::<&Recovery>()
            .iter(restored.world())
            .count(),
        2
    );
    assert!(
        restored
            .world_mut()
            .query::<&ModelRequest>()
            .iter(restored.world())
            .all(|request| request.status == WorkStatus::Cancelled)
    );
    assert!(
        restored
            .world_mut()
            .query::<&ToolUse>()
            .iter(restored.world())
            .all(|tool_use| tool_use.status == WorkStatus::Cancelled)
    );
    let recovered: Vec<_> = restored
        .world_mut()
        .query::<&Turn>()
        .iter(restored.world())
        .filter(|turn| turn.generation == 5 || turn.generation == 8)
        .map(|turn| turn.generation)
        .collect();
    let mut recovered = recovered;
    recovered.sort_unstable();
    assert_eq!(recovered, [5, 8]);
    restored.update();
    assert!(
        restored
            .world_mut()
            .query::<&ModelRequest>()
            .iter(restored.world())
            .all(|request| request.status == WorkStatus::Cancelled)
    );
    assert_eq!(
        restored
            .world_mut()
            .query::<&ModelResponse>()
            .iter(restored.world())
            .count(),
        0
    );
}

#[rstest]
fn commands_reject_before_harness_ready(unstarted_app: App) {
    let mut app = unstarted_app;
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
fn debounce_delays_first_snapshot_until_due(temp_dir: TempDir) {
    let path = temp_dir.path().join("storage");
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
}
