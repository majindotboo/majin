use assert_fs::{TempDir, prelude::*};
use bevy::prelude::*;
use majin::*;
use predicates::prelude::*;
use pretty_assertions::{assert_eq, assert_ne};
use proptest::prelude::*;
use proptest_derive::Arbitrary;
use rstest::rstest;
use static_assertions::assert_impl_all;
use std::{fs, io::Write, time::Duration};

use crate::persistence_support::*;

assert_impl_all!(PersistenceConfig: Clone, Send, Sync, std::fmt::Debug);

#[derive(Debug, Arbitrary)]
struct SessionPair {
    first: u64,
    second: u64,
}

proptest! {
    #[test]
    fn distinct_sessions_have_distinct_log_paths(pair in any::<SessionPair>()) {
        prop_assume!(pair.first != pair.second);
        prop_assert_ne!(
            session_log(std::path::Path::new("sessions"), SessionId(pair.first)),
            session_log(std::path::Path::new("sessions"), SessionId(pair.second)),
        );
    }
}

fn two_session_app(path: std::path::PathBuf) -> (App, Entity, Entity, Entity, Entity) {
    let mut app = app(path, Duration::ZERO);
    let first_session = app.world().resource::<ActiveSession>().0;
    let second_session = app
        .world_mut()
        .spawn(Session {
            id: SessionId(2),
            active_head: None,
        })
        .id();
    let foreign_turn = completed_turn(&mut app, second_session, "foreign");
    let local_turn = completed_turn(&mut app, first_session, "local");
    (app, first_session, second_session, foreign_turn, local_turn)
}

fn assert_invalid_log_is_preserved(path: &std::path::Path, invalid: &str) {
    let log = session_log(path, SessionId(1));
    let mut restored = app(path.to_path_buf(), Duration::ZERO);
    restored.update();
    assert_eq!(fs::read_to_string(log).expect("preserved log"), invalid);
    assert_eq!(
        restored
            .world_mut()
            .query::<&PersistenceFailure>()
            .iter(restored.world())
            .count(),
        1
    );
    assert_eq!(
        restored
            .world_mut()
            .query::<&Turn>()
            .iter(restored.world())
            .count(),
        0
    );
}

#[test]
fn default_persistence_path_is_the_session_directory() {
    let path = PersistenceConfig::default().path;
    assert!(
        path.is_absolute(),
        "default path is not user-scoped: {path:?}"
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
fn session_log_is_explicit_jsonl_and_excludes_ecs_runtime_state(temp_dir: TempDir) {
    let path = temp_dir.path().join("test-storage");
    let mut first = app(path.clone(), Duration::ZERO);
    let session = first.world().resource::<ActiveSession>().0;
    completed_turn(&mut first, session, "explicit schema");
    first.update();

    let source = fs::read_to_string(session_log(&path, SessionId(1))).expect("session log");
    assert!(source.contains("\"schema\":1"));
    assert!(source.contains("\"type\":\"Session\""));
    assert!(source.contains("\"type\":\"UserMessage\""));
    assert!(!source.contains("majin::"));
    assert!(!source.contains("TuiView"));
    assert!(!source.contains("entities"));
}

#[rstest]
fn session_log_is_written_as_a_filesystem_file(temp_dir: TempDir) {
    let path = temp_dir.path().join("sessions");
    let _app = app(path, Duration::ZERO);
    temp_dir
        .child("sessions/session-1.jsonl")
        .assert(predicate::path::is_file());
}

#[rstest]
fn cross_session_active_head_is_rejected_before_replay(temp_dir: TempDir) {
    let path = temp_dir.path().join("test-storage");
    let (mut first, first_session, _, foreign_turn, _) = two_session_app(path.clone());
    first
        .world_mut()
        .get_mut::<Session>(first_session)
        .expect("first session")
        .active_head = Some(foreign_turn);
    first.update();
    drop(first);

    let log = session_log(&path, SessionId(1));
    let invalid = fs::read_to_string(&log).expect("invalid log");
    assert!(session_log(&path, SessionId(2)).is_file());
    assert_invalid_log_is_preserved(&path, &invalid);
}

#[rstest]
fn cross_session_turn_parent_is_rejected_before_replay(temp_dir: TempDir) {
    let path = temp_dir.path().join("test-storage");
    let (mut first, _, _, foreign_turn, local_turn) = two_session_app(path.clone());
    first
        .world_mut()
        .get_mut::<Turn>(local_turn)
        .expect("local turn")
        .parent = Some(foreign_turn);
    first.update();
    drop(first);

    let invalid = fs::read_to_string(session_log(&path, SessionId(1))).expect("invalid log");
    assert_invalid_log_is_preserved(&path, &invalid);
}

#[rstest]
fn malformed_complete_record_is_preserved_and_blocks_replay(temp_dir: TempDir) {
    let path = temp_dir.path().join("test-storage");
    let mut first = app(path.clone(), Duration::ZERO);
    first.update();
    drop(first);
    let log = session_log(&path, SessionId(1));
    let mut source = fs::read_to_string(&log).expect("session log");
    source.push_str("not json\n");
    fs::write(&log, &source).expect("corrupt session log");

    assert_invalid_log_is_preserved(&path, &source);
}

#[rstest]
fn unsupported_schema_is_preserved_and_blocks_replay(temp_dir: TempDir) {
    let path = temp_dir.path().join("test-storage");
    let mut first = app(path.clone(), Duration::ZERO);
    first.update();
    drop(first);
    let log = session_log(&path, SessionId(1));
    let source = fs::read_to_string(&log).expect("session log");
    let invalid = source.replacen("\"schema\":1", "\"schema\":999", 1);
    assert_ne!(source, invalid);
    fs::write(&log, &invalid).expect("write unsupported schema");

    assert_invalid_log_is_preserved(&path, &invalid);
}

#[rstest]
fn incomplete_final_record_is_truncated_and_prior_history_replays(temp_dir: TempDir) {
    let path = temp_dir.path().join("test-storage");
    let mut first = app(path.clone(), Duration::ZERO);
    let session = first.world().resource::<ActiveSession>().0;
    let _turn = completed_turn(&mut first, session, "survives partial write");
    first.update();
    drop(first);
    let log = session_log(&path, SessionId(1));
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .expect("open session log");
    file.write_all(b"{\"schema\":1")
        .expect("append partial record");
    drop(file);

    let mut restored = app(path.clone(), Duration::ZERO);
    assert_eq!(
        restored
            .world_mut()
            .query::<&Turn>()
            .iter(restored.world())
            .count(),
        1
    );
    assert_eq!(
        restored
            .world_mut()
            .query::<&TurnCompleted>()
            .iter(restored.world())
            .count(),
        1
    );
    assert_eq!(
        restored
            .world_mut()
            .query::<&PersistenceFailure>()
            .iter(restored.world())
            .count(),
        0
    );
}
