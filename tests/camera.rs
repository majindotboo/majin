mod common;

use bevy::{
    ecs::system::Command,
    prelude::{App, Entity},
};
use majin::{
    ActiveSession, AssistantMessage, MessageId, SelectBranch, Sequence, Session, SubmitPrompt,
    TranscriptCamera, TranscriptRow, Turn, TurnId, UserMessage, project_transcript,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

use common::test_app;

const FAKE_RESPONSE: &str = "Fake harness received the message. No agent is connected yet.";

fn active_session(app: &App) -> Entity {
    app.world().resource::<ActiveSession>().0
}

use common::{
    ConversationPlan, ConversationStep, Harness, apply_conversation_plan, harness, prompt_text,
};

proptest! {
    #[test]
    fn selected_cameras_project_only_their_ancestral_facts(plan in any::<ConversationPlan>()) {
        let mut harness = Harness::disabled();
        let steps = apply_conversation_plan(&mut harness, &plan);

        for step in &steps {
            let expected = expected_branch_texts(step.turn, &steps);
            let transcript_camera = harness.app.world_mut().spawn(TranscriptCamera {
                session: harness.session,
                head: Some(step.turn),
            }).id();
            let transcript = project_transcript(harness.app.world_mut(), transcript_camera)
                .into_iter()
                .map(transcript_text)
                .collect::<Vec<_>>();
            assert_eq!(transcript, expected);

            let context_camera = harness.app.world_mut().spawn(ContextCamera {
                agent: harness.agent,
                session: harness.session,
                head: Some(step.turn),
                budget: usize::MAX,
            }).id();
            let context_users = project_context(harness.app.world_mut(), context_camera)
                .expect("valid context branch")
                .entries
                .into_iter()
                .map(|entry| context_text(&entry).to_owned())
                .collect::<Vec<_>>();
            assert_eq!(context_users, expected);
        }
    }

#[test]
fn transcript_projection_sorts_facts_by_sequence_and_id() {
    let mut app = test_app();
    let session = active_session(&app);
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
        id: MessageId(1),
        turn,
        sequence: Sequence(2),
        text: "first".into(),
    });
    app.world_mut().spawn(UserMessage {
        id: MessageId(2),
        turn,
        sequence: Sequence(2),
        text: "second".into(),
    });
    let camera = app
        .world_mut()
        .spawn(TranscriptCamera {
            session,
            head: Some(turn),
        })
        .id();

    assert_eq!(
        project_transcript(app.world_mut(), camera),
        [
            TranscriptRow::Assistant("first".into()),
            TranscriptRow::User("second".into()),
        ]
    );
}

#[rstest]
fn transcript_cameras_project_only_their_selected_branches(mut app: App) {
    let session = active_session(&app);
    let root = submit_prompt(&mut app, session, "root");
    let left = submit_prompt(&mut app, session, "left");
    SelectBranch {
        session,
        head: root,
    }
    .apply(app.world_mut());
    let right = submit_prompt(&mut app, session, "right");
    let left_camera = app
        .world_mut()
        .spawn(TranscriptCamera {
            session,
            head: Some(left),
        })
        .id();
    let right_camera = app
        .world_mut()
        .spawn(TranscriptCamera {
            session,
            head: Some(right),
        })
        .id();

    assert_eq!(
        project_transcript(app.world_mut(), left_camera),
        [
            TranscriptRow::User("root".into()),
            TranscriptRow::Assistant(FAKE_RESPONSE.into()),
            TranscriptRow::User("left".into()),
            TranscriptRow::Assistant(FAKE_RESPONSE.into()),
        ]
    );
    assert_eq!(
        project_transcript(app.world_mut(), right_camera),
        [
            TranscriptRow::User("root".into()),
            TranscriptRow::Assistant(FAKE_RESPONSE.into()),
            TranscriptRow::User("right".into()),
            TranscriptRow::Assistant(FAKE_RESPONSE.into()),
        ]
    );
}

#[test]
fn transcript_projection_reports_a_missing_parent() {
    let mut app = test_app();
    let session = active_session(&app);
    let missing_parent = app.world_mut().spawn_empty().id();
    app.world_mut().despawn(missing_parent);
    let head = app
        .world_mut()
        .spawn(Turn {
            id: TurnId(1),
            session,
            parent: Some(missing_parent),
            sequence: Sequence(1),
        })
        .id();
    let camera = app
        .world_mut()
        .spawn(TranscriptCamera {
            session,
            head: Some(head),
        })
        .id();

    assert_eq!(
        project_transcript(app.world_mut(), camera),
        [TranscriptRow::Error(
            "Transcript branch contains a missing Turn.".into(),
        )]
    );
}

#[rstest]
fn transcript_projection_reports_a_parent_cycle(mut app: App) {
    let session = active_session(&app);
    let first = app.world_mut().spawn_empty().id();
    let second = app.world_mut().spawn_empty().id();
    app.world_mut().entity_mut(first).insert(Turn {
        id: TurnId(1),
        session,
        parent: Some(second),
        sequence: Sequence(1),
    });
    app.world_mut().entity_mut(second).insert(Turn {
        id: TurnId(2),
        session,
        parent: Some(first),
        sequence: Sequence(2),
    });
    let camera = app
        .world_mut()
        .spawn(TranscriptCamera {
            session: context.session,
            head: context.head,
        })
        .id();
    assert_eq!(
        project_transcript(harness.app.world_mut(), transcript),
        [TranscriptRow::Error(message.into())]
    );
}
