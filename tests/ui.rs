use bevy::{app::Startup, prelude::*};
use bevy_ratatui::event::{KeyMessage, MouseMessage};
use majin::{
    ActiveSession, HarnessReady, ModelRequest, Sequence, Session, ToolUse, TranscriptCamera,
    TranscriptRow, TranscriptWork, TuiView, Turn, TurnCancelled, TurnCompleted, TurnId, WorkStatus,
    project_transcript,
};
use pretty_assertions::assert_eq;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use rstest::rstest;

pub mod common;

use common::{Harness, harness, single, unstarted_app, update_until};

#[rstest]
fn startup_creates_a_view_for_the_active_session(mut harness: Harness) {
    let view = view(&mut harness.app);
    let transcript_camera = harness
        .app
        .world()
        .get::<TuiView>(view)
        .expect("TUI view")
        .transcript_camera;
    let camera = harness
        .app
        .world()
        .get::<TranscriptCamera>(transcript_camera)
        .expect("transcript camera");
    assert_eq!(camera.session, harness.session);
    assert_eq!(camera.head, None);
    assert!(
        harness
            .app
            .world()
            .get::<majin::TerminalTranscriptViewport>(transcript_camera)
            .is_some()
    );
}

#[rstest]
fn keyboard_selection_updates_the_session_and_branch_camera(mut harness: Harness) {
    let first_session = harness.session;
    let second_session = harness.add_session(2);
    send_key(&mut harness.app, KeyCode::Tab, KeyModifiers::NONE);
    send_text(&mut harness.app, "second");
    send_key(&mut harness.app, KeyCode::Enter, KeyModifiers::NONE);
    harness.app.update();
    assert_eq!(
        harness.app.world().resource::<ActiveSession>().0,
        second_session
    );
    let second_head = harness
        .app
        .world()
        .get::<Session>(second_session)
        .unwrap()
        .active_head
        .expect("second session prompt");
    assert_eq!(
        harness
            .app
            .world()
            .get::<Turn>(second_head)
            .unwrap()
            .session,
        second_session
    );
    assert_eq!(
        harness
            .app
            .world_mut()
            .query::<&majin::UserMessage>()
            .iter(harness.app.world())
            .find(|message| message.turn == second_head)
            .expect("submitted user message")
            .text,
        "second"
    );

    update_until(&mut harness.app, |app| {
        app.world_mut()
            .query::<&TurnCompleted>()
            .iter(app.world())
            .any(|outcome| outcome.turn == second_head)
    });
    send_key(&mut harness.app, KeyCode::BackTab, KeyModifiers::SHIFT);
    harness.app.update();
    assert_eq!(
        harness.app.world().resource::<ActiveSession>().0,
        first_session
    );

    let root = harness
        .app
        .world_mut()
        .spawn(Turn {
            id: TurnId(100),
            session: first_session,
            parent: None,
            sequence: Sequence(100),
            generation: 0,
        })
        .id();
    let left = harness
        .app
        .world_mut()
        .spawn(Turn {
            id: TurnId(101),
            session: first_session,
            parent: Some(root),
            sequence: Sequence(101),
            generation: 0,
        })
        .id();
    let right = harness
        .app
        .world_mut()
        .spawn(Turn {
            id: TurnId(102),
            session: first_session,
            parent: Some(root),
            sequence: Sequence(102),
            generation: 0,
        })
        .id();
    harness
        .app
        .world_mut()
        .get_mut::<Session>(first_session)
        .unwrap()
        .active_head = Some(left);
    send_key(&mut harness.app, KeyCode::Down, KeyModifiers::CONTROL);
    harness.app.update();

    assert_eq!(
        harness
            .app
            .world()
            .get::<Session>(first_session)
            .unwrap()
            .active_head,
        Some(right)
    );
    let transcript_camera = camera(&mut harness.app);
    assert_eq!(
        harness
            .app
            .world()
            .get::<TranscriptCamera>(transcript_camera)
            .unwrap()
            .head,
        Some(right)
    );
}

#[rstest]
#[case::up(ScrollKey::Up, 1)]
#[case::page_up(ScrollKey::PageUp, 10)]
#[case::down(ScrollKey::Down, 4)]
#[case::page_down(ScrollKey::PageDown, 0)]
#[case::home(ScrollKey::Home, usize::MAX)]
#[case::end(ScrollKey::End, 0)]
fn keyboard_scroll_cases_update_only_tui_viewport(
    mut harness: Harness,
    #[case] key: ScrollKey,
    #[case] expected: usize,
) {
    let transcript_camera = camera(&mut harness.app);
    if matches!(key, ScrollKey::Down | ScrollKey::PageDown) {
        harness
            .app
            .world_mut()
            .get_mut::<majin::TerminalTranscriptViewport>(transcript_camera)
            .unwrap()
            .scroll_from_bottom = 5;
    }
    let (code, modifiers) = match key {
        ScrollKey::Up => (KeyCode::Up, KeyModifiers::NONE),
        ScrollKey::PageUp => (KeyCode::PageUp, KeyModifiers::NONE),
        ScrollKey::Down => (KeyCode::Down, KeyModifiers::NONE),
        ScrollKey::PageDown => (KeyCode::PageDown, KeyModifiers::NONE),
        ScrollKey::Home => (KeyCode::Home, KeyModifiers::NONE),
        ScrollKey::End => (KeyCode::End, KeyModifiers::NONE),
    };
    send_key(&mut harness.app, code, modifiers);
    harness.app.update();
    let actual = harness
        .app
        .world()
        .get::<majin::TerminalTranscriptViewport>(transcript_camera)
        .unwrap()
        .scroll_from_bottom;
    assert_eq!(actual, expected);
}

#[rstest]
fn queued_text_waits_in_the_composer_while_work_is_active(mut harness: Harness) {
    send_text(&mut harness.app, "first");
    send_key(&mut harness.app, KeyCode::Enter, KeyModifiers::NONE);
    send_text(&mut harness.app, "second");
    send_key(&mut harness.app, KeyCode::Enter, KeyModifiers::NONE);
    harness.app.update();

    let view = view(&mut harness.app);
    assert_eq!(
        harness.app.world().get::<TuiView>(view).unwrap().composer,
        "second"
    );
    assert_eq!(harness.count::<Turn>(), 1);
}

#[rstest]
fn active_work_is_visible_and_control_x_interrupts_only_that_turn(mut harness: Harness) {
    send_text(&mut harness.app, "interrupt");
    send_key(&mut harness.app, KeyCode::Enter, KeyModifiers::NONE);
    harness.app.update();
    let turn = harness.active_head().expect("active turn");
    let transcript_camera = camera(&mut harness.app);
    assert!(
        project_transcript(harness.app.world_mut(), transcript_camera)
            .iter()
            .any(|row| matches!(
                row,
                TranscriptRow::Work {
                    work: TranscriptWork::Model,
                    status: WorkStatus::Running,
                }
            ))
    );

    send_text(&mut harness.app, "queued");
    send_key(&mut harness.app, KeyCode::Enter, KeyModifiers::NONE);
    harness.app.update();
    let view = view(&mut harness.app);
    assert_eq!(
        harness.app.world().get::<TuiView>(view).unwrap().composer,
        "queued"
    );

    update_until(&mut harness.app, |app| {
        app.world_mut()
            .query::<&ToolUse>()
            .iter(app.world())
            .any(|tool| {
                tool.turn == turn
                    && matches!(tool.status, WorkStatus::Pending | WorkStatus::Running)
            })
    });
    assert!(
        project_transcript(harness.app.world_mut(), transcript_camera)
            .iter()
            .any(|row| matches!(
                row,
                TranscriptRow::Work {
                    work: TranscriptWork::Tool(tool),
                    status: WorkStatus::Pending | WorkStatus::Running,
                } if tool == "fake_tool"
            ))
    );

    send_key(&mut harness.app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    harness.app.update();
    assert!(
        harness
            .app
            .world_mut()
            .query::<&TurnCancelled>()
            .iter(harness.app.world())
            .any(|cancelled| cancelled.turn == turn)
    );
    assert!(
        harness
            .app
            .world_mut()
            .query::<&ModelRequest>()
            .iter(harness.app.world())
            .filter(|request| request.turn == turn)
            .all(|request| !matches!(request.status, WorkStatus::Pending | WorkStatus::Running))
    );
    assert!(
        harness
            .app
            .world_mut()
            .query::<&ToolUse>()
            .iter(harness.app.world())
            .filter(|tool| tool.turn == turn)
            .all(|tool| tool.status == WorkStatus::Cancelled)
    );
    assert!(
        project_transcript(harness.app.world_mut(), transcript_camera)
            .contains(&TranscriptRow::System("Turn cancelled.".into()))
    );
}

#[rstest]
fn mouse_scroll_changes_the_viewport_by_the_mouse_step(mut harness: Harness) {
    let transcript_camera = camera(&mut harness.app);
    harness
        .app
        .world_mut()
        .write_message(MouseMessage(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }));
    harness.app.update();
    assert_eq!(
        harness
            .app
            .world()
            .get::<majin::TerminalTranscriptViewport>(transcript_camera)
            .unwrap()
            .scroll_from_bottom,
        3
    );
    harness
        .app
        .world_mut()
        .write_message(MouseMessage(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }));
    harness.app.update();
    assert_eq!(
        harness
            .app
            .world()
            .get::<majin::TerminalTranscriptViewport>(transcript_camera)
            .unwrap()
            .scroll_from_bottom,
        0
    );
}

#[rstest]
fn loading_state_waits_for_harness_startup(mut unstarted_app: App) {
    unstarted_app.finish();
    unstarted_app.cleanup();
    assert!(!unstarted_app.world().contains_resource::<HarnessReady>());
    assert_eq!(
        unstarted_app
            .world_mut()
            .query_filtered::<Entity, With<TuiView>>()
            .iter(unstarted_app.world())
            .count(),
        0
    );

    unstarted_app.world_mut().run_schedule(Startup);
    let view = view(&mut unstarted_app);
    let transcript_camera = unstarted_app
        .world()
        .get::<TuiView>(view)
        .expect("TUI view")
        .transcript_camera;
    assert!(unstarted_app.world().contains_resource::<HarnessReady>());
    assert_eq!(
        unstarted_app
            .world()
            .get::<TranscriptCamera>(transcript_camera)
            .expect("transcript camera")
            .head,
        None
    );
}

#[rstest]
fn input_buffered_before_startup_submits_after_readiness(mut unstarted_app: App) {
    send_text(&mut unstarted_app, "buffered");
    send_key(&mut unstarted_app, KeyCode::Enter, KeyModifiers::NONE);

    unstarted_app.update();

    assert!(unstarted_app.world().contains_resource::<HarnessReady>());
    let session = unstarted_app.world().resource::<ActiveSession>().0;
    assert!(
        unstarted_app
            .world()
            .get::<Session>(session)
            .expect("active session")
            .active_head
            .is_some()
    );
}

#[derive(Debug, Clone, Copy)]
enum ScrollKey {
    Up,
    PageUp,
    Down,
    PageDown,
    Home,
    End,
}

fn send_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.world_mut()
        .write_message(KeyMessage(KeyEvent::new(code, modifiers)));
}

fn send_text(app: &mut App, text: &str) {
    for character in text.chars() {
        send_key(app, KeyCode::Char(character), KeyModifiers::NONE);
    }
}

fn view(app: &mut App) -> Entity {
    single::<TuiView>(app)
}

fn camera(app: &mut App) -> Entity {
    let view = view(app);
    app.world()
        .get::<TuiView>(view)
        .expect("TUI view")
        .transcript_camera
}
