use bevy::{
    ecs::system::Command,
    prelude::{App, Entity},
};
use bevy_ratatui::event::{KeyMessage, MouseMessage};
use majin::{
    ActiveSession, Agent, AgentTool, AssistantMessage, MajinPlugin, MessageId, Model, Provider,
    SelectSession, Sequence, Session, SessionId, SubmitPrompt, TerminalTranscriptViewport,
    ToolDefinition, TranscriptCamera, TranscriptRow, TuiView, Turn, TurnId, UserMessage,
    project_transcript,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MajinPlugin);
    app.update();
    app
}

fn single_entity<T: bevy::prelude::Component>(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, bevy::prelude::With<T>>()
        .single(app.world())
        .expect("one matching entity")
}

#[test]
fn startup_registers_harness_capabilities_and_session_view() {
    let mut app = test_app();

    let provider = single_entity::<Provider>(&mut app);
    let model = single_entity::<Model>(&mut app);
    let agent = single_entity::<Agent>(&mut app);
    let tool = single_entity::<ToolDefinition>(&mut app);
    let exposure = single_entity::<AgentTool>(&mut app);

    assert_eq!(app.world().get::<Model>(model).unwrap().provider, provider);
    assert_eq!(app.world().get::<Agent>(agent).unwrap().model, model);
    let exposure = app.world().get::<AgentTool>(exposure).unwrap();
    assert_eq!(exposure.agent, agent);
    assert_eq!(exposure.tool, tool);

    let session = single_entity::<Session>(&mut app);
    assert_eq!(app.world().resource::<ActiveSession>().0, session);

    let view = single_entity::<TuiView>(&mut app);
    let camera = app.world().get::<TuiView>(view).unwrap().transcript_camera;
    assert_eq!(
        app.world().get::<TranscriptCamera>(camera).unwrap().session,
        session
    );
}

#[test]
fn composer_queues_prompt_into_world_facts() {
    let mut app = test_app();

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

    let users: Vec<_> = app
        .world_mut()
        .query::<&UserMessage>()
        .iter(app.world())
        .cloned()
        .collect();
    let assistants: Vec<_> = app
        .world_mut()
        .query::<&AssistantMessage>()
        .iter(app.world())
        .cloned()
        .collect();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].text, "quit");
    assert_eq!(assistants.len(), 1);

    let session = app.world().resource::<ActiveSession>().0;
    let turn = single_entity::<Turn>(&mut app);
    assert_eq!(app.world().get::<Turn>(turn).unwrap().session, session);
    assert_eq!(
        app.world().get::<Session>(session).unwrap().active_head,
        Some(turn)
    );
    assert_eq!(users[0].turn, turn);
    assert_eq!(assistants[0].turn, turn);

    let camera = app.world().get::<TuiView>(view).unwrap().transcript_camera;
    let transcript = project_transcript(app.world_mut(), camera);
    assert_eq!(
        transcript,
        [
            TranscriptRow::User("quit".into()),
            TranscriptRow::Assistant(
                "Fake harness received the message. No agent is connected yet.".into()
            )
        ]
    );
}

#[test]
fn empty_prompt_creates_no_turn_or_message() {
    let mut app = test_app();
    let session = app.world().resource::<ActiveSession>().0;

    SubmitPrompt {
        session,
        text: "  ".into(),
    }
    .apply(app.world_mut());

    assert_eq!(
        app.world_mut().query::<&Turn>().iter(app.world()).count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&UserMessage>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&AssistantMessage>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn each_prompt_extends_the_linear_session_head() {
    let mut app = test_app();
    let session = app.world().resource::<ActiveSession>().0;

    SubmitPrompt {
        session,
        text: "first".into(),
    }
    .apply(app.world_mut());
    let first = app
        .world()
        .get::<Session>(session)
        .unwrap()
        .active_head
        .unwrap();

    SubmitPrompt {
        session,
        text: "second".into(),
    }
    .apply(app.world_mut());
    let second = app
        .world()
        .get::<Session>(session)
        .unwrap()
        .active_head
        .unwrap();

    assert_ne!(first, second);
    assert_eq!(app.world().get::<Turn>(second).unwrap().parent, Some(first));
}

#[test]
fn transcript_projection_sorts_facts_by_sequence_and_id() {
    let mut app = test_app();
    let session = app.world().resource::<ActiveSession>().0;
    let turn = app
        .world_mut()
        .spawn(Turn {
            id: TurnId(1),
            session,
            parent: None,
            sequence: Sequence(1),
        })
        .id();
    app.world_mut().spawn(AssistantMessage {
        id: MessageId(2),
        turn,
        sequence: Sequence(2),
        text: "second".into(),
    });
    app.world_mut().spawn(UserMessage {
        id: MessageId(1),
        turn,
        sequence: Sequence(2),
        text: "first".into(),
    });

    let camera = single_entity::<TranscriptCamera>(&mut app);
    let transcript = project_transcript(app.world_mut(), camera);

    assert_eq!(
        transcript,
        [
            TranscriptRow::User("first".into()),
            TranscriptRow::Assistant("second".into())
        ]
    );
}

#[test]
fn selecting_session_updates_active_transcript_camera() {
    let mut app = test_app();
    let session = app
        .world_mut()
        .spawn(Session {
            id: SessionId(2),
            active_head: None,
        })
        .id();

    SelectSession { session }.apply(app.world_mut());
    app.update();

    let camera = single_entity::<TranscriptCamera>(&mut app);
    assert_eq!(app.world().resource::<ActiveSession>().0, session);
    assert_eq!(
        app.world().get::<TranscriptCamera>(camera).unwrap().session,
        session
    );
}

#[test]
fn mouse_scroll_moves_terminal_viewport() {
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

    app.world_mut()
        .get_mut::<TerminalTranscriptViewport>(camera)
        .unwrap()
        .scroll_from_bottom = 3;
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
