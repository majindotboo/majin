use assert_fs::TempDir;
use bevy::ecs::system::Command;
use majin::*;
use pretty_assertions::{assert_eq, assert_ne};
use rstest::rstest;
use std::{fs, path::PathBuf, time::Duration};

use crate::common::build_test_app;
use crate::persistence_support::*;

#[rstest]
fn append_failure_preserves_prior_log_and_recovers(temp_dir: TempDir) {
    let path = temp_dir.path().join("test-storage");
    let mut app = app(path.clone(), Duration::ZERO);
    let log = session_log(&path, SessionId(1));
    let prior = fs::read_to_string(&log).expect("initial session log");
    let blocked = temp_dir.path().join("blocked");
    fs::write(&blocked, "not a directory").expect("create blocked path");
    app.world_mut().resource_mut::<PersistenceConfig>().path = blocked;

    let session = app.world().resource::<ActiveSession>().0;
    SubmitPrompt {
        session,
        text: "save failure".into(),
    }
    .apply(app.world_mut());
    app.update();
    assert_eq!(fs::read_to_string(&log).unwrap(), prior);
    assert_eq!(
        app.world_mut()
            .query::<&PersistenceFailure>()
            .iter(app.world())
            .count(),
        1
    );

    app.world_mut().resource_mut::<PersistenceConfig>().path = storage_root(&path);
    app.update();
    assert_ne!(fs::read_to_string(&log).unwrap(), prior);
}

#[cfg(unix)]
#[rstest]
fn append_log_keeps_owner_only_permissions(temp_dir: TempDir) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let path = temp_dir.path().join("test-storage");
    let mut app = app(path.clone(), Duration::ZERO);
    let log = session_log(&path, SessionId(1));
    fs::set_permissions(&log, fs::Permissions::from_mode(0o644)).expect("widen log mode");
    let session = app.world().resource::<ActiveSession>().0;
    SubmitPrompt {
        session,
        text: "replace log".into(),
    }
    .apply(app.world_mut());
    app.update();

    assert_eq!(fs::metadata(&log).unwrap().mode() & 0o777, 0o600);
}

#[rstest]
fn relative_persistence_directory_saves(temp_dir: TempDir) {
    let root = PathBuf::from("target").join(temp_dir.path().file_name().unwrap());
    let app = app(root.clone(), Duration::ZERO);
    drop(build_test_app());
    assert!(session_log(&root, SessionId(1)).is_file());
    drop(app);
    fs::remove_dir_all(root).expect("remove relative temp state");
}
