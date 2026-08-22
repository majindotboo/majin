use bevy::{
    ecs::{message::Messages, system::Command},
    prelude::*,
};
use majin::{
    ActiveSession, CommandFailure, CommandResult, InterruptTurn, SubmitPrompt, Turn, UserMessage,
};
use pretty_assertions::{assert_eq, assert_ne};
use proptest::prelude::*;
use rstest::rstest;

pub mod common;

use common::{
    ConversationPlan, Harness, apply_conversation_plan_without_execution, assert_capability_graph,
    harness, prompt_text, whitespace_text,
};

proptest! {
    #[test]
    fn generated_prompt_traces_preserve_branch_parents_and_user_history(
        plan in any::<ConversationPlan>()
    ) {
        let mut harness = Harness::disabled();
        let steps = apply_conversation_plan_without_execution(&mut harness, &plan);

        assert_eq!(harness.active_head(), steps.last().map(|step| step.turn));
        assert_eq!(harness.count::<Turn>(), plan.prompts.len());
        assert_eq!(harness.count::<UserMessage>(), plan.prompts.len());

        for step in &steps {
            let turn = harness.app.world().get::<Turn>(step.turn).expect("planned turn");
            assert_eq!(turn.session, harness.session);
            assert_eq!(turn.parent, step.parent);
            let message = harness
                .app
                .world_mut()
                .query::<&UserMessage>()
                .iter(harness.app.world())
                .find(|message| message.turn == step.turn)
                .expect("one user message per turn");
            assert_eq!(message.text, step.text);
        }

        let mut sequences = harness
            .app
            .world_mut()
            .query::<&Turn>()
            .iter(harness.app.world())
            .map(|turn| turn.sequence.0)
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert!(sequences.windows(2).all(|window| window[0] < window[1]));
        sequences.dedup();
        assert_eq!(sequences.len(), plan.prompts.len());
    }

    #[test]
    fn generated_prompt_validation_rejects_whitespace_and_trims_text(
        whitespace in whitespace_text(),
        text in prompt_text(),
    ) {
        let mut harness = Harness::disabled();
        let mut cursor = harness
            .app
            .world()
            .resource::<Messages<CommandResult>>()
            .get_cursor_current();

        SubmitPrompt {
            session: harness.session,
            text: whitespace,
        }
        .apply(harness.app.world_mut());

        assert_eq!(harness.count::<Turn>(), 0);
        assert_eq!(harness.count::<UserMessage>(), 0);
        assert_eq!(harness.count::<majin::ModelRequest>(), 0);
        assert_eq!(
            cursor
                .read(harness.app.world().resource::<Messages<CommandResult>>())
                .cloned()
                .collect::<Vec<_>>(),
            [CommandResult::PromptRejected {
                session: harness.session,
                failure: CommandFailure::EmptyPrompt,
            }]
        );

        let turn = harness.submit(format!("  {text}  "));
        let message = harness
            .app
            .world_mut()
            .query::<&UserMessage>()
            .iter(harness.app.world())
            .find(|message| message.turn == turn)
            .expect("submitted user message");
        assert_eq!(message.text, text);
    }
}

#[rstest]
fn startup_builds_a_linked_capability_graph_and_one_active_session(mut harness: Harness) {
    assert_capability_graph(&mut harness.app);
    assert_eq!(
        harness.app.world().resource::<ActiveSession>().0,
        harness.session
    );
    assert_eq!(harness.active_head(), None);
}

#[rstest]
fn invalid_entity_commands_emit_their_typed_rejections(mut harness: Harness) {
    let missing = harness.app.world_mut().spawn_empty().id();
    harness.app.world_mut().despawn(missing);
    let mut cursor = harness
        .app
        .world()
        .resource::<Messages<CommandResult>>()
        .get_cursor_current();

    SubmitPrompt {
        session: missing,
        text: "missing".into(),
    }
    .apply(harness.app.world_mut());
    majin::SelectSession { session: missing }.apply(harness.app.world_mut());
    majin::SelectBranch {
        session: missing,
        head: missing,
    }
    .apply(harness.app.world_mut());
    InterruptTurn { turn: missing }.apply(harness.app.world_mut());

    assert_eq!(
        cursor
            .read(harness.app.world().resource::<Messages<CommandResult>>())
            .cloned()
            .collect::<Vec<_>>(),
        [
            CommandResult::PromptRejected {
                session: missing,
                failure: CommandFailure::MissingSession,
            },
            CommandResult::SessionRejected {
                session: missing,
                failure: CommandFailure::MissingSession,
            },
            CommandResult::BranchRejected {
                session: missing,
                head: missing,
                failure: CommandFailure::MissingTurn,
            },
            CommandResult::InterruptRejected {
                turn: missing,
                failure: CommandFailure::MissingTurn,
            },
        ]
    );
}

#[rstest]
fn active_and_foreign_turn_guards_leave_the_selected_session_unchanged(mut harness: Harness) {
    let finished = harness.submit("finished");
    harness.complete(finished);
    let active = harness.submit("active");
    let mut cursor = harness
        .app
        .world()
        .resource::<Messages<CommandResult>>()
        .get_cursor_current();

    SubmitPrompt {
        session: harness.session,
        text: "blocked".into(),
    }
    .apply(harness.app.world_mut());
    majin::SelectBranch {
        session: harness.session,
        head: finished,
    }
    .apply(harness.app.world_mut());
    assert_eq!(harness.active_head(), Some(active));
    assert_eq!(
        cursor
            .read(harness.app.world().resource::<Messages<CommandResult>>())
            .cloned()
            .collect::<Vec<_>>(),
        [
            CommandResult::PromptRejected {
                session: harness.session,
                failure: CommandFailure::ActiveTurn,
            },
            CommandResult::BranchRejected {
                session: harness.session,
                head: finished,
                failure: CommandFailure::ActiveTurn,
            },
        ]
    );

    harness.complete(active);
    let other = harness.add_session(2);
    let foreign = {
        let previous = harness.session;
        harness.session = other;
        let turn = harness.submit("foreign");
        harness.complete(turn);
        harness.session = previous;
        turn
    };
    majin::SelectBranch {
        session: harness.session,
        head: foreign,
    }
    .apply(harness.app.world_mut());

    assert_eq!(harness.active_head(), Some(active));
    assert_ne!(harness.active_head(), Some(foreign));
}
