use bevy::prelude::*;
use majin::{
    ActiveSession, Agent, AssistantMessage, Compaction, ContextCamera, ContextDocument,
    ContextEntry, MessageId, Model, ModelChange, ProjectionError, Recovery, SelectBranch, Sequence,
    Session, SubmitPrompt, ToolCallId, ToolOutcome, ToolUse, TranscriptCamera, TranscriptRow, Turn,
    TurnCompleted, TurnId, UserMessage, WorkStatus, project_context, project_transcript,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

pub mod common;

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
    fn transcript_order_is_sequence_then_id_then_kind(
        specs in proptest::collection::vec((1u8..=8u8, prompt_text(), any::<bool>()), 1..=12)
    ) {
        let mut harness = Harness::disabled();
        let turn = spawn_turn(&mut harness.app, harness.session, 1, None, 1);
        for (index, (_, text, assistant)) in specs.iter().enumerate() {
            if *assistant {
                harness.app.world_mut().spawn(AssistantMessage {
                    id: MessageId(index as u64 + 1),
                    turn,
                    sequence: Sequence(specs[index].0 as u64),
                    text: text.clone(),
                });
            } else {
                harness.app.world_mut().spawn(UserMessage {
                    id: MessageId(index as u64 + 1),
                    turn,
                    sequence: Sequence(specs[index].0 as u64),
                    text: text.clone(),
                });
            }
        }
        let camera = harness.app.world_mut().spawn(TranscriptCamera {
            session: harness.session,
            head: Some(turn),
        }).id();
        let mut expected = specs.iter().enumerate().collect::<Vec<_>>();
        expected.sort_by_key(|(index, (sequence, _, assistant))| {
            (*sequence, *index as u64 + 1, *assistant)
        });
        let expected = expected
            .into_iter()
            .map(|(_, (_, text, assistant))| {
                if *assistant {
                    TranscriptRow::Assistant(text.clone())
                } else {
                    TranscriptRow::User(text.clone())
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(project_transcript(harness.app.world_mut(), camera), expected);
    }

    #[test]
    fn context_fact_ordering_is_stable_for_generated_fact_mixes(
        specs in proptest::collection::vec((1u8..=8u8, prompt_text(), 0u8..=4u8), 1..=12)
    ) {
        let mut harness = Harness::disabled();
        let turn = spawn_turn(&mut harness.app, harness.session, 1, None, 1);
        let mut expected = Vec::with_capacity(specs.len());
        for (index, (sequence, text, kind)) in specs.iter().enumerate() {
            let id = index as u64 + 1;
            let (id_key, kind_key, entry) = match kind {
                0 => {
                    harness.app.world_mut().spawn(UserMessage {
                        id: MessageId(id),
                        turn,
                        sequence: Sequence(*sequence as u64),
                        text: text.clone(),
                    });
                    (id, 0, ContextEntry::User(text.clone()))
                }
                1 => {
                    harness.app.world_mut().spawn(AssistantMessage {
                        id: MessageId(id),
                        turn,
                        sequence: Sequence(*sequence as u64),
                        text: text.clone(),
                    });
                    (id, 1, ContextEntry::Assistant(text.clone()))
                }
                2 => {
                    harness.app.world_mut().spawn(ModelChange {
                        turn,
                        model: harness.model,
                        sequence: Sequence(*sequence as u64),
                    });
                    (0, 4, ContextEntry::ModelChange("fake-model".into()))
                }
                3 => {
                    harness.app.world_mut().spawn(Compaction {
                        turn,
                        summary: text.clone(),
                        sequence: Sequence(*sequence as u64),
                    });
                    (0, 5, ContextEntry::Compaction(text.clone()))
                }
                4 => {
                    harness.app.world_mut().spawn(Recovery {
                        turn,
                        text: text.clone(),
                        sequence: Sequence(*sequence as u64),
                    });
                    (0, 6, ContextEntry::Recovery(text.clone()))
                }
                _ => unreachable!(),
            };
            expected.push((*sequence, id_key, kind_key, entry));
        }
        expected.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then_with(|| context_text(&left.3).cmp(context_text(&right.3)))
        });
        let camera = harness.app.world_mut().spawn(ContextCamera {
            agent: harness.agent,
            session: harness.session,
            head: Some(turn),
            budget: usize::MAX,
        }).id();
        assert_eq!(
            project_context(harness.app.world_mut(), camera),
            Ok(ContextDocument {
                entries: expected.into_iter().map(|(_, _, _, entry)| entry).collect(),
            })
        );
    }

    #[test]
    fn context_budget_keeps_only_ordered_entries_that_fit(
        texts in proptest::collection::vec(prompt_text(), 1..=6),
        budget in 0u8..=64u8,
    ) {
        let mut harness = Harness::disabled();
        let turn = spawn_turn(&mut harness.app, harness.session, 1, None, 1);
        for (index, text) in texts.iter().enumerate() {
            harness.app.world_mut().spawn(UserMessage {
                id: MessageId(index as u64 + 1),
                turn,
                sequence: Sequence(index as u64 + 2),
                text: text.clone(),
            });
        }
        let camera = harness.app.world_mut().spawn(ContextCamera {
            agent: harness.agent,
            session: harness.session,
            head: Some(turn),
            budget: budget as usize,
        }).id();
        let entries = project_context(harness.app.world_mut(), camera)
            .expect("valid budget projection")
            .entries;
        let all = texts
            .iter()
            .cloned()
            .map(ContextEntry::User)
            .collect::<Vec<_>>();
        let mut expected = Vec::new();
        let mut remaining = budget as usize;
        for entry in all.iter().rev() {
            let size = context_size(entry);
            if size <= remaining {
                remaining -= size;
                expected.push(entry.clone());
            }
        }
        expected.reverse();
        assert_eq!(entries, expected);
    }
}

#[rstest]
fn transcript_projection_reports_a_missing_parent(mut app: App) {
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
            generation: 0,
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
#[case::missing_parent(
    BrokenProjection::MissingParent,
    "Transcript branch contains a missing Turn."
)]
#[case::cycle(BrokenProjection::Cycle, "Transcript branch contains a cycle.")]
fn transcript_projection_explains_broken_branches(
    mut harness: Harness,
    #[case] broken: BrokenProjection,
    #[case] message: &str,
) {
    let context_camera = broken_context_camera(&mut harness, broken);
    let context = *harness
        .app
        .world()
        .get::<ContextCamera>(context_camera)
        .unwrap();
    let transcript = harness
        .app
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
