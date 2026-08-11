use bevy::prelude::*;
use majin::{
    AssistantMessage, Compaction, ContextCamera, ContextDocument, ContextEntry, MessageId,
    ModelChange, ModelRequest, PersistenceFailure, ProjectionError, ProviderFailure, Recovery,
    RecoveryFailure, Sequence, ToolFailure, ToolUse, TranscriptCamera, TranscriptRow,
    TranscriptWork, Turn, TurnFailure, TurnId, TurnInterrupted, UserMessage, WorkStatus,
    project_context, project_transcript,
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
#[case::missing_camera(BrokenProjection::MissingCamera, ProjectionError::MissingCamera)]
#[case::wrong_camera(BrokenProjection::WrongCamera, ProjectionError::MissingCamera)]
#[case::missing_parent(BrokenProjection::MissingParent, ProjectionError::MissingTurn)]
#[case::cycle(BrokenProjection::Cycle, ProjectionError::Cycle)]
#[case::cross_session(BrokenProjection::CrossSession, ProjectionError::CrossSession)]
#[case::missing_model(BrokenProjection::MissingModel, ProjectionError::MissingModel)]
fn context_projection_reports_the_structured_invariant_failure(
    mut harness: Harness,
    #[case] broken: BrokenProjection,
    #[case] expected: ProjectionError,
) {
    let camera = broken_context_camera(&mut harness, broken);
    assert_eq!(
        project_context(harness.app.world_mut(), camera),
        Err(expected)
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

#[rstest]
#[case::model(ActiveWork::Model)]
#[case::tool(ActiveWork::Tool)]
fn transcript_projects_each_kind_of_active_work(mut harness: Harness, #[case] work: ActiveWork) {
    let turn = spawn_turn(&mut harness.app, harness.session, 1, None, 1);
    let camera = harness
        .app
        .world_mut()
        .spawn(TranscriptCamera {
            session: harness.session,
            head: Some(turn),
        })
        .id();
    match work {
        ActiveWork::Model => {
            harness.app.world_mut().spawn(ModelRequest {
                id: majin::ModelRequestId(1),
                turn,
                agent: harness.agent,
                model: harness.model,
                provider: harness.provider,
                generation: 0,
                previous_tool_use: None,
                status: WorkStatus::Running,
                sequence: Sequence(2),
            });
            assert_eq!(
                project_transcript(harness.app.world_mut(), camera),
                [TranscriptRow::Work {
                    work: TranscriptWork::Model,
                    status: WorkStatus::Running,
                }]
            );
        }
        ActiveWork::Tool => {
            harness.app.world_mut().spawn(ToolUse {
                id: majin::ToolCallId(1),
                turn,
                agent: harness.agent,
                tool: harness.tool,
                model: harness.model,
                provider: harness.provider,
                generation: 0,
                input: "input".into(),
                status: WorkStatus::Pending,
                sequence: Sequence(2),
            });
            assert_eq!(
                project_transcript(harness.app.world_mut(), camera),
                [
                    TranscriptRow::ToolUse {
                        tool: "fake_tool".into(),
                        input: "input".into(),
                    },
                    TranscriptRow::Work {
                        work: TranscriptWork::Tool("fake_tool".into()),
                        status: WorkStatus::Pending,
                    },
                ]
            );
        }
    }
}

#[rstest]
fn transcript_orders_failure_recovery_and_persistence_outcomes(mut harness: Harness) {
    let provider_turn = spawn_turn(&mut harness.app, harness.session, 201, None, 1);
    harness.app.world_mut().spawn(majin::TurnFailed {
        turn: provider_turn,
        generation: 0,
        failure: TurnFailure::Provider(ProviderFailure {
            message: "provider failed".into(),
        }),
        sequence: Sequence(2),
    });
    let tool_turn = spawn_turn(
        &mut harness.app,
        harness.session,
        202,
        Some(provider_turn),
        3,
    );
    harness.app.world_mut().spawn(majin::TurnFailed {
        turn: tool_turn,
        generation: 0,
        failure: TurnFailure::Tool(ToolFailure {
            message: "tool failed".into(),
        }),
        sequence: Sequence(4),
    });
    let recovery_failure_turn =
        spawn_turn(&mut harness.app, harness.session, 203, Some(tool_turn), 5);
    harness.app.world_mut().spawn(majin::TurnFailed {
        turn: recovery_failure_turn,
        generation: 0,
        failure: TurnFailure::Recovery(RecoveryFailure {
            message: "recovery failed".into(),
        }),
        sequence: Sequence(6),
    });
    let cancelled_turn = spawn_turn(
        &mut harness.app,
        harness.session,
        204,
        Some(recovery_failure_turn),
        7,
    );
    harness.app.world_mut().spawn(majin::TurnCancelled {
        turn: cancelled_turn,
        generation: 1,
        sequence: Sequence(8),
    });
    let interrupted_turn = spawn_turn(
        &mut harness.app,
        harness.session,
        205,
        Some(cancelled_turn),
        9,
    );
    harness.app.world_mut().spawn(TurnInterrupted {
        turn: interrupted_turn,
        generation: 2,
        sequence: Sequence(10),
    });
    harness.app.world_mut().spawn(Recovery {
        turn: interrupted_turn,
        text: "Recovered interrupted work.".into(),
        sequence: Sequence(11),
    });
    harness.app.world_mut().spawn(PersistenceFailure {
        message: "save failed".into(),
        sequence: Sequence(12),
    });
    let camera = harness
        .app
        .world_mut()
        .spawn(TranscriptCamera {
            session: harness.session,
            head: Some(interrupted_turn),
        })
        .id();

    assert_eq!(
        project_transcript(harness.app.world_mut(), camera),
        [
            TranscriptRow::Error("Provider: provider failed".into()),
            TranscriptRow::Error("Tool: tool failed".into()),
            TranscriptRow::Error("Recovery: recovery failed".into()),
            TranscriptRow::System("Turn cancelled.".into()),
            TranscriptRow::System("Turn interrupted during recovery.".into()),
            TranscriptRow::System("Recovered interrupted work.".into()),
            TranscriptRow::Error("Persistence: save failed".into()),
        ]
    );
}

fn ancestry(head: Entity, steps: &[ConversationStep]) -> Vec<&ConversationStep> {
    let mut branch = Vec::new();
    let mut current = Some(head);
    while let Some(turn) = current {
        let step = steps
            .iter()
            .find(|step| step.turn == turn)
            .expect("planned turn");
        branch.push(step);
        current = step.parent;
    }
    branch.reverse();
    branch
}

fn expected_branch_texts(head: Entity, steps: &[ConversationStep]) -> Vec<String> {
    ancestry(head, steps)
        .into_iter()
        .flat_map(|step| {
            [
                step.text.clone(),
                "Calling fake_tool.".into(),
                step.text.clone(),
                format!("Fake tool completed: {}", step.text),
                "Fake harness completed the request.".into(),
            ]
        })
        .collect()
}

fn spawn_turn(
    app: &mut App,
    session: Entity,
    id: u64,
    parent: Option<Entity>,
    sequence: u64,
) -> Entity {
    app.world_mut()
        .spawn(Turn {
            id: TurnId(id),
            session,
            parent,
            sequence: Sequence(sequence),
            generation: 0,
        })
        .id()
}

fn context_text(entry: &ContextEntry) -> &str {
    match entry {
        ContextEntry::User(text)
        | ContextEntry::Assistant(text)
        | ContextEntry::ModelChange(text)
        | ContextEntry::Compaction(text)
        | ContextEntry::Recovery(text) => text,
        ContextEntry::ToolUse { input, .. } => input,
        ContextEntry::ToolOutcome { output, .. } => output,
    }
}

fn transcript_text(row: TranscriptRow) -> String {
    match row {
        TranscriptRow::User(text)
        | TranscriptRow::Assistant(text)
        | TranscriptRow::System(text)
        | TranscriptRow::Error(text) => text,
        TranscriptRow::ToolUse { input, .. } => input,
        TranscriptRow::ToolOutcome { output, .. } => output,
        TranscriptRow::Work { .. } => "active work".into(),
    }
}

fn context_size(entry: &ContextEntry) -> usize {
    context_text(entry).len()
}

#[derive(Debug, Clone, Copy)]
enum BrokenProjection {
    MissingCamera,
    WrongCamera,
    MissingParent,
    Cycle,
    CrossSession,
    MissingModel,
}

#[derive(Debug, Clone, Copy)]
enum ActiveWork {
    Model,
    Tool,
}

fn broken_context_camera(harness: &mut Harness, broken: BrokenProjection) -> Entity {
    if matches!(broken, BrokenProjection::MissingCamera) {
        let camera = harness.app.world_mut().spawn_empty().id();
        harness.app.world_mut().despawn(camera);
        return camera;
    }
    if matches!(broken, BrokenProjection::WrongCamera) {
        return harness
            .app
            .world_mut()
            .spawn(TranscriptCamera {
                session: harness.session,
                head: None,
            })
            .id();
    }

    let (head, session) = match broken {
        BrokenProjection::MissingParent => {
            let missing = harness.app.world_mut().spawn_empty().id();
            harness.app.world_mut().despawn(missing);
            (
                spawn_turn(&mut harness.app, harness.session, 1, Some(missing), 1),
                harness.session,
            )
        }
        BrokenProjection::Cycle => {
            let first = harness.app.world_mut().spawn_empty().id();
            let second = harness.app.world_mut().spawn_empty().id();
            harness.app.world_mut().entity_mut(first).insert(Turn {
                id: TurnId(1),
                session: harness.session,
                parent: Some(second),
                sequence: Sequence(1),
                generation: 0,
            });
            harness.app.world_mut().entity_mut(second).insert(Turn {
                id: TurnId(2),
                session: harness.session,
                parent: Some(first),
                sequence: Sequence(2),
                generation: 0,
            });
            (first, harness.session)
        }
        BrokenProjection::CrossSession => {
            let foreign = harness
                .app
                .world_mut()
                .spawn(majin::Session {
                    id: majin::SessionId(2),
                    active_head: None,
                })
                .id();
            (
                spawn_turn(&mut harness.app, foreign, 1, None, 1),
                harness.session,
            )
        }
        BrokenProjection::MissingModel => {
            let turn = spawn_turn(&mut harness.app, harness.session, 1, None, 1);
            let missing = harness.app.world_mut().spawn_empty().id();
            harness.app.world_mut().despawn(missing);
            harness.app.world_mut().spawn(ModelChange {
                turn,
                model: missing,
                sequence: Sequence(2),
            });
            (turn, harness.session)
        }
        BrokenProjection::MissingCamera | BrokenProjection::WrongCamera => unreachable!(),
    };
    harness
        .app
        .world_mut()
        .spawn(ContextCamera {
            agent: harness.agent,
            session,
            head: Some(head),
            budget: usize::MAX,
        })
        .id()
}
