use bevy::prelude::App;
use bevy_ratatui::event::{KeyMessage, MouseMessage};
use majin::{MajinPlugin, TuiView};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MajinPlugin);
    app
}

#[test]
fn composer_submits_a_fake_exchange() {
    let mut app = test_app();
    let initial_items = app.world().resource::<TuiView>().transcript.len();

    for character in "quit".chars() {
        app.world_mut().write_message(KeyMessage(KeyEvent::new(
            KeyCode::Char(character),
            KeyModifiers::NONE,
        )));
    }
    app.world_mut().write_message(KeyMessage(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));

    app.update();

    let view = app.world().resource::<TuiView>();
    assert!(view.composer.is_empty());
    assert_eq!(view.transcript.len(), initial_items + 2);
    assert_eq!(view.transcript[initial_items].body, "quit");
}

#[test]
fn mouse_scroll_moves_through_transcript() {
    let mut app = test_app();

    app.world_mut().write_message(MouseMessage(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
    app.update();
    assert_eq!(app.world().resource::<TuiView>().scroll_from_bottom, 3);

    let mut app = test_app();
    app.world_mut().resource_mut::<TuiView>().scroll_from_bottom = 3;
    app.world_mut().write_message(MouseMessage(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
    app.update();
    assert_eq!(app.world().resource::<TuiView>().scroll_from_bottom, 0);
}
