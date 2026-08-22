// Shared support is compiled into independent targets, each using a different subset.
#![allow(dead_code)]

use std::{path::PathBuf, time::Duration};

use assert_fs::TempDir;
use bevy::prelude::{App, Component, Entity, With};
use majin::{PersistenceConfig, Session, SessionId, SubmitPrompt, TurnCompleted};
use rstest::fixture;

use crate::common::update_until;

#[fixture]
pub fn temp_dir() -> TempDir {
    TempDir::new().expect("temporary directory")
}

pub fn app(path: PathBuf, debounce: Duration) -> App {
    let mut app = App::new();
    app.insert_resource(PersistenceConfig {
        path: storage_root(&path),
        debounce,
        enabled: true,
    });
    app.add_plugins(majin::MajinPlugin);
    app.update();
    app
}

pub fn app_with_disposables(path: PathBuf, debounce: Duration, count: usize) -> (App, Vec<Entity>) {
    let mut app = App::new();
    app.insert_resource(PersistenceConfig {
        path: storage_root(&path),
        debounce,
        enabled: true,
    });
    let disposables = (0..count)
        .map(|_| app.world_mut().spawn_empty().id())
        .collect();
    app.add_plugins(majin::MajinPlugin);
    app.update();
    (app, disposables)
}

pub fn storage_root(path: &std::path::Path) -> PathBuf {
    path.parent()
        .filter(|_| path.extension().is_some())
        .map_or_else(|| path.to_path_buf(), PathBuf::from)
}

pub fn session_log(path: &std::path::Path, session: SessionId) -> PathBuf {
    storage_root(path).join(format!("session-{}.jsonl", session.0))
}

pub fn single<T: Component>(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<T>>()
        .single(app.world())
        .expect("one entity")
}

pub fn completed_turn(app: &mut App, session: Entity, text: &str) -> Entity {
    use bevy::ecs::system::Command;

    SubmitPrompt {
        session,
        text: text.into(),
    }
    .apply(app.world_mut());
    let turn = app
        .world()
        .get::<Session>(session)
        .unwrap()
        .active_head
        .unwrap();
    update_until(app, |app| {
        app.world_mut()
            .query::<&TurnCompleted>()
            .iter(app.world())
            .any(|outcome| outcome.turn == turn)
    });
    turn
}
