use std::{fs, io::Write, path::Path, time::Duration};

use assert_fs::{TempDir, prelude::*};
use bevy::{ecs::system::Command, prelude::*};
use majin::{
    ContextCamera, HarnessReady, ModelRequest, PersistenceConfig, PersistenceFailure,
    PersistentContextCamera, Recovery, Session, SessionId, SubmitPrompt, ToolUse, Turn,
    TurnCompleted, TurnId, TurnInterrupted, WorkStatus,
};
use predicates::path::is_file;
use pretty_assertions::{assert_eq, assert_ne};
use proptest::prelude::*;
use proptest_derive::Arbitrary;
use rstest::rstest;
use serde_json::Value;

pub mod common;

use common::{
    ConversationPlan, Harness, apply_conversation_plan, read_log, session_log, single, temp_dir,
    unstarted_app,
};

proptest! {
    #![proptest_config(ProptestConfig {
        // Keep 16 independent session allocations while each generated case stays cheap.
        cases: 16,
        ..ProptestConfig::default()
    })]
    #[test]
    fn each_session_gets_a_distinct_append_only_file(ids in any::<AlternateSession>()) {
        let temp_dir = TempDir::new().expect("temporary directory");
        let root = temp_dir.path().join("storage");
        let mut harness = Harness::persistent(root.clone(), Duration::from_secs(60));
        harness.add_session(ids.id);
        harness.flush_persistence();

        let first_log = session_log(&root, SessionId(1));
        let second_log = session_log(&root, SessionId(ids.id));
        temp_dir.child("storage/session-1.jsonl").assert(is_file());
        temp_dir
            .child(format!("storage/session-{}.jsonl", ids.id))
            .assert(is_file());
        assert_ne!(first_log, second_log);
        assert!(read_log(&root, SessionId(ids.id)).contains(&format!("\"id\":{}", ids.id)));
    }
}

#[rstest]
fn default_storage_is_user_scoped_and_session_partitioned() {
    let path = PersistenceConfig::default().path;
    assert!(
        path.is_absolute(),
        "default persistence path is not absolute: {path:?}"
    );
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("sessions")
    );
    assert_eq!(
        path.parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str()),
        Some(".majin")
    );
}

#[rstest]
fn session_logs_are_explicit_jsonl_and_exclude_runtime_state(temp_dir: TempDir) {
    let root = temp_dir.path().join("storage");
    let mut harness = Harness::persistent(root.clone(), Duration::ZERO);
    let turn = harness.submit("explicit schema");
    harness.complete_with_fake_results(turn);
    let log = session_log(&root, SessionId(1));
    temp_dir.child("storage/session-1.jsonl").assert(is_file());

    let source = read_log(&root, SessionId(1));
    let records = source
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("valid JSONL record"))
        .collect::<Vec<_>>();
    assert!(
        !records.is_empty(),
        "session log should contain records: {log:?}"
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record["schema"].as_u64().expect("schema version"))
            .collect::<Vec<_>>(),
        vec![1; records.len()]
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record["ordinal"].as_u64().expect("record ordinal"))
            .collect::<Vec<_>>(),
        (0..records.len() as u64).collect::<Vec<_>>()
    );
    for excluded in [
        "TuiView",
        "TerminalTranscriptViewport",
        "entities",
        "majin::",
    ] {
        assert!(
            !source.contains(excluded),
            "runtime value leaked into JSONL: {excluded}"
        );
    }
}

proptest! {
    // Keep 16 restart/replay boundaries while each case performs one durable flush.
    #![proptest_config(ProptestConfig {
        cases: 16,
        ..ProptestConfig::default()
    })]
    #[test]
    fn replaying_a_generated_conversation_preserves_the_canonical_log(
        plan in any::<ConversationPlan>()
    ) {
        let temp_dir = TempDir::new().expect("temporary directory");
        let root = temp_dir.path().join("storage");
        let mut first = Harness::persistent(root.clone(), Duration::from_secs(60));
        let steps = apply_conversation_plan(&mut first, &plan);
        let view = single::<majin::TuiView>(&mut first.app);
        first.app.world_mut().get_mut::<majin::TuiView>(view).unwrap().composer = "transient".into();
        let transcript_camera = first.app.world().get::<majin::TuiView>(view).unwrap().transcript_camera;
        first.app.world_mut().get_mut::<majin::TerminalTranscriptViewport>(transcript_camera).unwrap().scroll_from_bottom = 7;
        first.flush_persistence();
        let before = read_log(&root, SessionId(1));
        let max_turn = first
            .app
            .world_mut()
            .query::<&Turn>()
            .iter(first.app.world())
            .map(|turn| turn.id.0)
            .max()
            .expect("generated turns");
        drop(first);

        let mut restored = Harness::persistent(root.clone(), Duration::from_secs(60));
        assert_eq!(read_log(&root, SessionId(1)), before);
        assert_eq!(restored.count::<Turn>(), steps.len());
        let restored_view = single::<majin::TuiView>(&mut restored.app);
        assert_eq!(restored.app.world().get::<majin::TuiView>(restored_view).unwrap().composer, "");
        let restored_camera = restored.app.world().get::<majin::TuiView>(restored_view).unwrap().transcript_camera;
        assert_eq!(
            restored
                .app
                .world()
                .get::<majin::TuiView>(restored_view)
                .unwrap()
                .focus,
            majin::TuiFocus::Composer
        );
        assert_eq!(restored.app.world().get::<majin::TerminalTranscriptViewport>(restored_camera).unwrap().scroll_from_bottom, 0);

    let continued = restored.submit("continued");
    assert!(restored.app.world().get::<Turn>(continued).unwrap().id.0 > max_turn);
    }
}

#[rstest]
fn persistent_context_budget_is_restored_and_used_for_the_next_request(temp_dir: TempDir) {
    let root = temp_dir.path().join("storage");
    let mut first = Harness::persistent(root.clone(), Duration::ZERO);
    let camera = first
        .app
        .world_mut()
        .query_filtered::<Entity, With<PersistentContextCamera>>()
        .single(first.app.world())
        .expect("persistent context camera");
    first
        .app
        .world_mut()
        .get_mut::<ContextCamera>(camera)
        .unwrap()
        .budget = 0;
    first.app.update();
    drop(first);

    let mut restored = Harness::persistent(root, Duration::ZERO);
    let camera = restored
        .app
        .world_mut()
        .query_filtered::<Entity, With<PersistentContextCamera>>()
        .single(restored.app.world())
        .expect("restored context camera");
    assert_eq!(
        restored
            .app
            .world()
            .get::<ContextCamera>(camera)
            .unwrap()
            .budget,
        0
    );
    let turn = restored.submit("budget excluded");
    restored.complete_with_fake_results(turn);
    assert!(
        restored
            .app
            .world_mut()
            .query::<&ToolUse>()
            .iter(restored.app.world())
            .any(|tool| tool.turn == turn && tool.input.is_empty())
    );
}

fn invalid_log(root: &Path, corruption: Corruption) -> String {
    match corruption {
        Corruption::MalformedCompleteRecord => {
            let _ = Harness::persistent(root.to_path_buf(), Duration::ZERO);
            let log = session_log(root, SessionId(1));
            let mut source = fs::read_to_string(&log).expect("initial log");
            source.push_str("not json\n");
            fs::write(&log, &source).expect("write malformed record");
            source
        }
        Corruption::UnsupportedSchema => {
            let _ = Harness::persistent(root.to_path_buf(), Duration::ZERO);
            let log = session_log(root, SessionId(1));
            let source = fs::read_to_string(&log).expect("initial log");
            let invalid = source.replacen("\"schema\":1", "\"schema\":999", 1);
            assert_ne!(source, invalid);
            fs::write(&log, &invalid).expect("write unsupported schema");
            invalid
        }
        Corruption::CrossSessionHead | Corruption::CrossSessionParent => {
            let mut harness = Harness::persistent(root.to_path_buf(), Duration::from_secs(60));
            let original_session = harness.session;
            let foreign_session = harness.add_session(2);
            harness.session = foreign_session;
            let foreign_turn = harness.submit("foreign");
            harness.complete_with_fake_results(foreign_turn);
            harness.session = original_session;
            let local_turn = harness.submit("local");
            harness.complete_with_fake_results(local_turn);
            match corruption {
                Corruption::CrossSessionHead => {
                    harness
                        .app
                        .world_mut()
                        .get_mut::<Session>(original_session)
                        .unwrap()
                        .active_head = Some(foreign_turn);
                }
                Corruption::CrossSessionParent => {
                    harness
                        .app
                        .world_mut()
                        .get_mut::<Turn>(local_turn)
                        .unwrap()
                        .parent = Some(foreign_turn);
                }
                Corruption::MalformedCompleteRecord | Corruption::UnsupportedSchema => {
                    unreachable!()
                }
            }
            harness.flush_persistence();
            read_log(root, SessionId(1))
        }
    }
}

fn assert_invalid_log_is_preserved(root: &Path, invalid: &str) {
    let mut restored = Harness::persistent(root.to_path_buf(), Duration::ZERO);
    assert_eq!(read_log(root, SessionId(1)), invalid);
    assert_eq!(restored.count::<PersistenceFailure>(), 1);
    assert_eq!(restored.count::<Turn>(), 0);
}

#[rstest]
#[case::malformed(Corruption::MalformedCompleteRecord)]
#[case::unsupported_schema(Corruption::UnsupportedSchema)]
#[case::cross_session_head(Corruption::CrossSessionHead)]
#[case::cross_session_parent(Corruption::CrossSessionParent)]
fn complete_invalid_records_are_preserved_and_block_replay(
    temp_dir: TempDir,
    #[case] corruption: Corruption,
) {
    let root = temp_dir.path().join("storage");
    let invalid = invalid_log(&root, corruption);
    assert_invalid_log_is_preserved(&root, &invalid);
}

#[rstest]
fn incomplete_final_records_are_truncated_while_prior_history_replays(temp_dir: TempDir) {
    let root = temp_dir.path().join("storage");
    let mut first = Harness::persistent(root.clone(), Duration::ZERO);
    let turn = first.submit("survives partial write");
    first.complete_with_fake_results(turn);
    let log = session_log(&root, SessionId(1));
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .expect("open log");
    file.write_all(b"{\"schema\":1")
        .expect("append partial record");
    drop(file);

    let mut restored = Harness::persistent(root.clone(), Duration::ZERO);
    assert_eq!(restored.count::<Turn>(), 1);
    assert_eq!(restored.count::<TurnCompleted>(), 1);
    assert_eq!(restored.count::<PersistenceFailure>(), 0);
    assert!(read_log(&root, SessionId(1)).ends_with('\n'));
}

#[rstest]
#[case::model(OrphanedWork::Model)]
#[case::tool(OrphanedWork::Tool)]
fn restart_cancels_each_kind_of_orphaned_work_once(temp_dir: TempDir, #[case] kind: OrphanedWork) {
    let root = temp_dir.path().join("storage");
    let mut first = Harness::persistent(root.clone(), Duration::ZERO);
    let turn = first
        .app
        .world_mut()
        .spawn(Turn {
            id: TurnId(99),
            session: first.session,
            parent: None,
            sequence: majin::Sequence(99),
            generation: 4,
        })
        .id();
    match kind {
        OrphanedWork::Model => {
            first.app.world_mut().spawn(ModelRequest {
                id: majin::ModelRequestId(99),
                turn,
                agent: first.agent,
                model: first.model,
                provider: first.provider,
                generation: 4,
                previous_tool_use: None,
                status: WorkStatus::Pending,
                sequence: majin::Sequence(100),
            });
        }
        OrphanedWork::Tool => {
            first.app.world_mut().spawn(ToolUse {
                id: majin::ToolCallId(99),
                turn,
                agent: first.agent,
                tool: first.tool,
                model: first.model,
                provider: first.provider,
                generation: 4,
                input: "orphan".into(),
                status: WorkStatus::Pending,
                sequence: majin::Sequence(100),
            });
        }
    }
    first.app.update();
    drop(first);

    let mut restored = Harness::persistent(root, Duration::ZERO);
    assert_eq!(restored.count::<TurnInterrupted>(), 1);
    assert_eq!(restored.count::<Recovery>(), 1);
    let restored_turn = restored
        .app
        .world_mut()
        .query::<&Turn>()
        .iter(restored.app.world())
        .find(|value| value.id == TurnId(99))
        .expect("restored orphaned turn");
    assert_eq!(restored_turn.generation, 5);
    assert!(
        restored
            .app
            .world_mut()
            .query::<&ModelRequest>()
            .iter(restored.app.world())
            .all(|request| request.status == WorkStatus::Cancelled)
    );
    assert!(
        restored
            .app
            .world_mut()
            .query::<&ToolUse>()
            .iter(restored.app.world())
            .all(|tool| tool.status == WorkStatus::Cancelled)
    );
    restored.app.update();
    assert_eq!(restored.count::<TurnInterrupted>(), 1);
}

#[rstest]
fn debounce_delays_the_first_snapshot_until_the_configured_deadline(temp_dir: TempDir) {
    let root = temp_dir.path().join("storage");
    let mut harness = Harness::persistent(root.clone(), Duration::from_secs(60));
    assert!(harness.app.world().contains_resource::<HarnessReady>());
    assert!(!session_log(&root, SessionId(1)).exists());
    harness
        .app
        .world_mut()
        .resource_mut::<PersistenceConfig>()
        .debounce = Duration::ZERO;
    harness.app.update();
    temp_dir.child("storage/session-1.jsonl").assert(is_file());
}

#[rstest]
fn append_failure_preserves_the_last_good_log_and_recovers(temp_dir: TempDir) {
    let root = temp_dir.path().join("storage");
    let mut harness = Harness::persistent(root.clone(), Duration::ZERO);
    let prior = read_log(&root, SessionId(1));
    let blocked = temp_dir.path().join("blocked");
    fs::write(&blocked, "not a directory").expect("blocked path");
    harness
        .app
        .world_mut()
        .resource_mut::<PersistenceConfig>()
        .path = blocked;
    harness.submit("save failure");
    harness.app.update();
    assert_eq!(read_log(&root, SessionId(1)), prior);
    assert_eq!(harness.count::<PersistenceFailure>(), 1);

    harness
        .app
        .world_mut()
        .resource_mut::<PersistenceConfig>()
        .path = root.clone();
    harness.app.update();
    assert_ne!(read_log(&root, SessionId(1)), prior);
}

#[cfg(unix)]
#[rstest]
fn appending_a_log_restores_owner_only_permissions(temp_dir: TempDir) {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir.path().join("storage");
    let mut harness = Harness::persistent(root.clone(), Duration::ZERO);
    let log = session_log(&root, SessionId(1));
    fs::set_permissions(&log, fs::Permissions::from_mode(0o644)).expect("widen log mode");
    harness.submit("replace log");
    harness.app.update();
    assert_eq!(
        fs::metadata(log).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[rstest]
fn commands_are_rejected_until_harness_startup_finishes(mut unstarted_app: App) {
    let session = unstarted_app.world_mut().spawn_empty().id();
    let mut cursor = unstarted_app
        .world()
        .resource::<bevy::ecs::message::Messages<majin::CommandResult>>()
        .get_cursor_current();
    SubmitPrompt {
        session,
        text: "blocked".into(),
    }
    .apply(unstarted_app.world_mut());
    assert_eq!(
        cursor
            .read(
                unstarted_app
                    .world()
                    .resource::<bevy::ecs::message::Messages<majin::CommandResult>>()
            )
            .cloned()
            .collect::<Vec<_>>(),
        [majin::CommandResult::PromptRejected {
            session,
            failure: majin::CommandFailure::HarnessNotReady,
        }]
    );
}

#[derive(Debug, Clone, Copy, Arbitrary)]
struct AlternateSession {
    #[proptest(strategy = "2u64..=10_000u64")]
    id: u64,
}

#[derive(Debug, Clone, Copy)]
enum Corruption {
    MalformedCompleteRecord,
    UnsupportedSchema,
    CrossSessionHead,
    CrossSessionParent,
}

#[derive(Debug, Clone, Copy, Arbitrary)]
enum OrphanedWork {
    Model,
    Tool,
}
