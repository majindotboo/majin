mod common;

use bevy::{
    ecs::system::Command,
    prelude::{Component, Entity, With},
};
use bevy_ratatui::event::{KeyMessage, MouseMessage};
use majin::{
    ActiveSession, AssistantMessage, SelectSession, Sequence, Session, SessionId,
    TerminalTranscriptViewport, TranscriptCamera, TranscriptRow, TuiView, Turn, TurnId,
    UserMessage, project_transcript,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use common::test_app;

const FAKE_RESPONSE: &str = "Fake harness received the message. No agent is connected yet.";

fn single_entity<T: Component>(app: &mut bevy::prelude::App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<T>>()
        .single(app.world())
        .expect("one matching entity")
}

fn active_session(app: &bevy::prelude::App) -> Entity {
    app.world().resource::<ActiveSession>().0
}

#[test]
fn startup_creates_a_terminal_view_for_the_active_session() {
    let mut app = test_app();
    let session = active_session(&app);
    let view = single_entity::<TuiView>(&mut app);
    let camera = app.world().get::<TuiView>(view).unwrap().transcript_camera;

    assert_eq!(
        app.world().get::<TranscriptCamera>(camera).unwrap().session,
        session
    );
    assert_eq!(
        app.world().get::<TranscriptCamera>(camera).unwrap().head,
        None
    );
    assert!(
        app.world()
            .get::<TerminalTranscriptViewport>(camera)
            .is_some()
    );
}

#[rstest]
fn composer_submits_a_prompt_and_advances_the_camera(mut app: App) {
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

    let view = single_entity::<TuiView>(&mut app);
    assert!(
        app.world()
            .get::<TuiView>(view)
            .unwrap()
            .composer
            .is_empty()
    );
    let turn = app
        .world_mut()
        .query_filtered::<Entity, With<Turn>>()
        .single(app.world())
        .unwrap();
    let camera = app.world().get::<TuiView>(view).unwrap().transcript_camera;
    assert_eq!(
        app.world().get::<TranscriptCamera>(camera).unwrap().head,
        Some(turn)
    );
    assert_eq!(
        project_transcript(app.world_mut(), camera),
        [
            TranscriptRow::User("quit".into()),
            TranscriptRow::Assistant(FAKE_RESPONSE.into()),
        ]
    );

    let user = app
        .world_mut()
        .query::<&UserMessage>()
        .single(app.world())
        .unwrap()
        .clone();
    let assistant = app
        .world_mut()
        .query::<&AssistantMessage>()
        .single(app.world())
        .unwrap()
        .clone();
    assert_eq!(user.turn, turn);
    assert_eq!(assistant.turn, turn);
}

#[test]
fn selecting_a_session_updates_the_transcript_camera() {
    let mut app = test_app();
    let session = app
        .world_mut()
        .spawn(Session {
            id: SessionId(2),
            active_head: None,
        })
        .id();
    let head = app
        .world_mut()
        .spawn(Turn {
            id: TurnId(1),
            session,
            parent: None,
            sequence: Sequence(1),
        })
        .id();
    app.world_mut()
        .get_mut::<Session>(session)
        .unwrap()
        .active_head = Some(head);

    SelectSession { session }.apply(app.world_mut());
    app.update();

    let camera = single_entity::<TranscriptCamera>(&mut app);
    assert_eq!(app.world().resource::<ActiveSession>().0, session);
    let camera = app.world().get::<TranscriptCamera>(camera).unwrap();
    assert_eq!(camera.session, session);
    assert_eq!(camera.head, Some(head));
}

#[test]
fn mouse_scroll_moves_the_terminal_viewport() {
    let mut app = test_app();
    let camera = single_entity::<TranscriptCamera>(&mut app);
    app.world_mut().write_message(MouseMessage(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
    app.update();
    assert_eq!(
        app.world()
            .get::<TerminalTranscriptViewport>(camera)
            .unwrap()
            .scroll_from_bottom,
        3
    );

    app.world_mut().write_message(MouseMessage(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
    app.update();
    assert_eq!(
        app.world()
            .get::<TerminalTranscriptViewport>(camera)
            .unwrap()
            .scroll_from_bottom,
        0
    );
}
