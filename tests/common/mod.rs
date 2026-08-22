use bevy::prelude::App;
use majin::MajinPlugin;

pub fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MajinPlugin);
    app.update();
    app
}
