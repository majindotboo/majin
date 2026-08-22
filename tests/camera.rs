use bevy::prelude::*;
use majin::{
    Agent, AssistantMessage, Compaction, ContextCamera, ContextDocument, ContextEntry, MessageId,
    Model, ModelChange, ModelRequest, PersistenceFailure, ProjectionError, Provider,
    ProviderFailure, Recovery, RecoveryFailure, Sequence, Session, SessionId, ToolCallId,
    ToolDefinition, ToolFailure, ToolOutcome, ToolUse, TranscriptCamera, TranscriptRow,
    TranscriptWork, Turn, TurnFailure, TurnId, TurnInterrupted, UserMessage, WorkStatus,
    project_context, project_transcript,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

pub mod common;

use common::{ConversationPlan, Harness, harness, prompt_text};

proptest! {
    #[test]
    fn selected_cameras_project_only_their_ancestral_facts(plan in any::<ConversationPlan>()) {
        let mut fixture = projection_fixture(&plan);
        let transcript_camera = fixture.world.spawn(TranscriptCamera {
            session: fixture.session,
            head: None,
        }).id();
        let context_camera = fixture.world.spawn(ContextCamera {
            agent: fixture.agent,
            session: fixture.session,
            head: None,
            budget: usize::MAX,
        }).id();

        for branch in &fixture.branches {
            fixture
                .world
                .get_mut::<TranscriptCamera>(transcript_camera)
                .expect("transcript camera")
                .head = Some(branch.head);
            assert_eq!(
                project_transcript(&mut fixture.world, transcript_camera),
                branch.transcript
            );
            fixture
                .world
                .get_mut::<ContextCamera>(context_camera)
                .expect("context camera")
                .head = Some(branch.head);
            assert_eq!(
                project_context(&mut fixture.world, context_camera),
                Ok(ContextDocument {
                    entries: branch.context.clone(),
                })
            );
        }
    }

    #[test]
    fn transcript_order_is_sequence_then_id_then_kind(
        specs in proptest::collection::vec((1u8..=8u8, prompt_text(), any::<bool>()), 1..=12)
    ) {
        let mut fixture = projection_world();
        let turn = spawn_turn_in_world(&mut fixture.world, fixture.session, 1, None, 1);
        for (index, (_, text, assistant)) in specs.iter().enumerate() {
            if *assistant {
                fixture.world.spawn(AssistantMessage {
                    id: MessageId(index as u64 + 1),
                    turn,
                    sequence: Sequence(specs[index].0 as u64),
                    text: text.clone(),
                });
            } else {
                fixture.world.spawn(UserMessage {
                    id: MessageId(index as u64 + 1),
                    turn,
                    sequence: Sequence(specs[index].0 as u64),
                    text: text.clone(),
                });
            }
        }
        let camera = fixture.world.spawn(TranscriptCamera {
            session: fixture.session,
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
        assert_eq!(project_transcript(&mut fixture.world, camera), expected);
    }

    #[test]
    fn context_fact_ordering_is_stable_for_generated_fact_mixes(
        specs in proptest::collection::vec((1u8..=8u8, prompt_text(), 0u8..=4u8), 1..=12)
    ) {
        let mut fixture = projection_world();
        let turn = spawn_turn_in_world(&mut fixture.world, fixture.session, 1, None, 1);
        let mut expected = Vec::with_capacity(specs.len());
        for (index, (sequence, text, kind)) in specs.iter().enumerate() {
            let id = index as u64 + 1;
            let (id_key, kind_key, entry) = match kind {
                0 => {
                    fixture.world.spawn(UserMessage {
                        id: MessageId(id),
                        turn,
                        sequence: Sequence(*sequence as u64),
                        text: text.clone(),
                    });
                    (id, 0, ContextEntry::User(text.clone()))
                }
                1 => {
                    fixture.world.spawn(AssistantMessage {
                        id: MessageId(id),
                        turn,
                        sequence: Sequence(*sequence as u64),
                        text: text.clone(),
                    });
                    (id, 1, ContextEntry::Assistant(text.clone()))
                }
                2 => {
                    fixture.world.spawn(ModelChange {
                        turn,
                        model: fixture.model,
                        sequence: Sequence(*sequence as u64),
                    });
                    (0, 4, ContextEntry::ModelChange("fake-model".into()))
                }
                3 => {
                    fixture.world.spawn(Compaction {
                        turn,
                        summary: text.clone(),
                        sequence: Sequence(*sequence as u64),
                    });
                    (0, 5, ContextEntry::Compaction(text.clone()))
                }
                4 => {
                    fixture.world.spawn(Recovery {
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
        let camera = fixture.world.spawn(ContextCamera {
            agent: fixture.agent,
            session: fixture.session,
            head: Some(turn),
            budget: usize::MAX,
        }).id();
        assert_eq!(
            project_context(&mut fixture.world, camera),
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
        let mut fixture = projection_world();
        let turn = spawn_turn_in_world(&mut fixture.world, fixture.session, 1, None, 1);
        for (index, text) in texts.iter().enumerate() {
            fixture.world.spawn(UserMessage {
                id: MessageId(index as u64 + 1),
                turn,
                sequence: Sequence(index as u64 + 2),
                text: text.clone(),
            });
        }
        let camera = fixture.world.spawn(ContextCamera {
            agent: fixture.agent,
            session: fixture.session,
            head: Some(turn),
            budget: budget as usize,
        }).id();
        let entries = project_context(&mut fixture.world, camera)
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

struct ProjectionFixture {
    world: World,
    session: Entity,
    agent: Entity,
    branches: Vec<ProjectionBranch>,
}

struct ProjectionWorld {
    world: World,
    session: Entity,
    agent: Entity,
    model: Entity,
}

struct ProjectionBranch {
    head: Entity,
    transcript: Vec<TranscriptRow>,
    context: Vec<ContextEntry>,
}

struct ProjectionStep {
    turn: Entity,
    parent: Option<Entity>,
    text: String,
    tool_call_id: ToolCallId,
}

fn projection_world() -> ProjectionWorld {
    let mut world = World::new();
    let session = world
        .spawn(Session {
            id: SessionId(1),
            active_head: None,
        })
        .id();
    let provider = world
        .spawn(Provider {
            provider_id: majin::ProviderId(1),
        })
        .id();
    let model = world
        .spawn(Model {
            provider,
            model_id: "fake-model".into(),
        })
        .id();
    let agent = world.spawn(Agent { model }).id();
    ProjectionWorld {
        world,
        session,
        agent,
        model,
    }
}

fn projection_fixture(plan: &ConversationPlan) -> ProjectionFixture {
    let ProjectionWorld {
        mut world,
        session,
        agent,
        model,
    } = projection_world();
    let provider = world
        .get::<Model>(model)
        .expect("projection model")
        .provider;
    let tool = world
        .spawn(ToolDefinition {
            tool_id: majin::ToolId(1),
            name: "fake_tool".into(),
            description: "Temporary fake tool capability.".into(),
        })
        .id();

    let mut root = None;
    let mut head = None;
    let mut steps = Vec::with_capacity(plan.prompts.len());
    for (index, text) in plan.prompts.iter().enumerate() {
        if index > 0 && plan.branch_to_root.get(index - 1).copied().unwrap_or(false) {
            head = root;
        }
        let parent = head;
        let id = index as u64 + 1;
        let base = id * 10;
        let turn = world
            .spawn(Turn {
                id: TurnId(id),
                session,
                parent,
                sequence: Sequence(base),
                generation: 0,
            })
            .id();
        let tool_call_id = ToolCallId(id);
        world.spawn(UserMessage {
            id: MessageId(base + 1),
            turn,
            sequence: Sequence(base + 1),
            text: text.clone(),
        });
        world.spawn(AssistantMessage {
            id: MessageId(base + 2),
            turn,
            sequence: Sequence(base + 2),
            text: "Calling fake_tool.".into(),
        });
        let tool_use = world
            .spawn(ToolUse {
                id: tool_call_id,
                turn,
                agent,
                tool,
                model,
                provider,
                generation: 0,
                input: text.clone(),
                status: WorkStatus::Succeeded,
                sequence: Sequence(base + 3),
            })
            .id();
        world.spawn(ToolOutcome {
            tool_use,
            tool_call_id,
            turn,
            generation: 0,
            output: format!("Fake tool completed: {text}"),
            sequence: Sequence(base + 4),
        });
        world.spawn(AssistantMessage {
            id: MessageId(base + 5),
            turn,
            sequence: Sequence(base + 5),
            text: "Fake harness completed the request.".into(),
        });
        root.get_or_insert(turn);
        head = Some(turn);
        steps.push(ProjectionStep {
            turn,
            parent,
            text: text.clone(),
            tool_call_id,
        });
    }

    let branches = steps
        .iter()
        .map(|step| {
            let mut chain = Vec::new();
            let mut current = Some(step.turn);
            while let Some(turn) = current {
                let ancestor = steps
                    .iter()
                    .find(|candidate| candidate.turn == turn)
                    .expect("projection step");
                chain.push(ancestor);
                current = ancestor.parent;
            }
            chain.reverse();

            let mut transcript = Vec::new();
            let mut context = Vec::new();
            for step in chain {
                transcript.extend([
                    TranscriptRow::User(step.text.clone()),
                    TranscriptRow::Assistant("Calling fake_tool.".into()),
                    TranscriptRow::ToolUse {
                        tool: "fake_tool".into(),
                        input: step.text.clone(),
                    },
                    TranscriptRow::ToolOutcome {
                        tool: "fake_tool".into(),
                        output: format!("Fake tool completed: {}", step.text),
                    },
                    TranscriptRow::Assistant("Fake harness completed the request.".into()),
                ]);
                context.extend([
                    ContextEntry::User(step.text.clone()),
                    ContextEntry::Assistant("Calling fake_tool.".into()),
                    ContextEntry::ToolUse {
                        tool_call_id: step.tool_call_id,
                        tool: "fake_tool".into(),
                        input: step.text.clone(),
                    },
                    ContextEntry::ToolOutcome {
                        tool_call_id: step.tool_call_id,
                        tool: "fake_tool".into(),
                        output: format!("Fake tool completed: {}", step.text),
                    },
                    ContextEntry::Assistant("Fake harness completed the request.".into()),
                ]);
            }
            ProjectionBranch {
                head: step.turn,
                transcript,
                context,
            }
        })
        .collect();

    ProjectionFixture {
        world,
        session,
        agent,
        branches,
    }
}

fn spawn_turn_in_world(
    world: &mut World,
    session: Entity,
    id: u64,
    parent: Option<Entity>,
    sequence: u64,
) -> Entity {
    world
        .spawn(Turn {
            id: TurnId(id),
            session,
            parent,
            sequence: Sequence(sequence),
            generation: 0,
        })
        .id()
}
