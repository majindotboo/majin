mod assertions;
mod plans;
mod runtime;
mod storage;

use bevy::prelude::App;
use majin::MajinPlugin;

pub fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MajinPlugin);
    update_until(&mut app, |_| true);
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
