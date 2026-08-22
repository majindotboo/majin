// Shared support is compiled into independent targets, each using a different subset.
#![allow(dead_code)]

use std::{
    thread::yield_now,
    time::{Duration, Instant},
};

use bevy::prelude::App;
use majin::{MajinPlugin, PersistenceConfig};
use rstest::fixture;

pub fn build_test_app() -> App {
    let mut app = App::new();
    app.insert_resource(PersistenceConfig::disabled());
    app.add_plugins(MajinPlugin);
    update_until(&mut app, |_| true);
    app
}

#[fixture]
pub fn app() -> App {
    build_test_app()
}

#[fixture]
pub fn unstarted_app() -> App {
    let mut app = App::new();
    app.insert_resource(PersistenceConfig::disabled());
    app.add_plugins(MajinPlugin);
    app
}

pub fn update_until(app: &mut App, mut predicate: impl FnMut(&mut App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        if predicate(app) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "condition did not complete before deadline"
        );
        yield_now();
    }
}
