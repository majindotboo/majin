use std::collections::{HashMap, HashSet};

use bevy::{
    ecs::{entity::MapEntities, reflect::ReflectMapEntities, system::SystemParam},
    prelude::*,
};

use crate::harness::{
    AssistantMessage, Compaction, Model, ModelChange, ModelRequest, PersistenceFailure, Recovery,
    Sequence, ToolDefinition, ToolOutcome, ToolUse, Turn, TurnCancelled, TurnFailed, TurnFailure,
    TurnInterrupted, UserMessage, WorkStatus,
};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, _app: &mut App) {}
}

#[derive(Component, Debug, Clone, Copy)]
pub struct TranscriptCamera {
    pub session: Entity,
    pub head: Option<Entity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptWork {
    Model,
    Tool(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptRow {
    User(String),
    Assistant(String),
    ToolUse {
        tool: String,
        input: String,
    },
    ToolOutcome {
        tool: String,
        output: String,
    },
    Work {
        work: TranscriptWork,
        status: WorkStatus,
    },
    System(String),
    Error(String),
}

#[derive(Component, Debug, Clone, Copy, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct ContextCamera {
    #[entities]
    pub agent: Entity,
    #[entities]
    pub session: Entity,
    #[entities]
    pub head: Option<Entity>,
    pub budget: usize,
}

#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct PersistentContextCamera;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextDocument {
    pub entries: Vec<ContextEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextEntry {
    User(String),
    Assistant(String),
    ToolUse {
        tool_call_id: crate::ToolCallId,
        tool: String,
        input: String,
    },
    ToolOutcome {
        tool_call_id: crate::ToolCallId,
        tool: String,
        output: String,
    },
    ModelChange(String),
    Compaction(String),
    Recovery(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionError {
    MissingCamera,
    MissingTurn,
    CrossSession,
    Cycle,
    MissingModel,
}

#[derive(SystemParam)]
pub(crate) struct TranscriptProjector<'w, 's> {
    turns: Query<'w, 's, (Entity, &'static Turn)>,
    users: Query<'w, 's, &'static UserMessage>,
    assistants: Query<'w, 's, &'static AssistantMessage>,
    model_requests: Query<'w, 's, &'static ModelRequest>,
    tool_uses: Query<'w, 's, (Entity, &'static ToolUse)>,
    tool_outcomes: Query<'w, 's, &'static ToolOutcome>,
    tools: Query<'w, 's, (Entity, &'static ToolDefinition)>,
    failures: Query<'w, 's, &'static TurnFailed>,
    cancellations: Query<'w, 's, &'static TurnCancelled>,
    interruptions: Query<'w, 's, &'static TurnInterrupted>,
    recoveries: Query<'w, 's, &'static Recovery>,
    persistence_failures: Query<'w, 's, &'static PersistenceFailure>,
}

struct TranscriptFacts<'a> {
    turns: Vec<(Entity, &'a Turn)>,
    users: Vec<&'a UserMessage>,
    assistants: Vec<&'a AssistantMessage>,
    model_requests: Vec<&'a ModelRequest>,
    tool_uses: Vec<(Entity, &'a ToolUse)>,
    tool_outcomes: Vec<&'a ToolOutcome>,
    tools: Vec<(Entity, &'a ToolDefinition)>,
    failures: Vec<&'a TurnFailed>,
    cancellations: Vec<&'a TurnCancelled>,
    interruptions: Vec<&'a TurnInterrupted>,
    recoveries: Vec<&'a Recovery>,
    persistence_failures: Vec<&'a PersistenceFailure>,
}

impl TranscriptProjector<'_, '_> {
    pub(crate) fn project(&self, camera: &TranscriptCamera) -> Vec<TranscriptRow> {
        project_transcript_parts(
            *camera,
            TranscriptFacts {
                turns: self.turns.iter().collect(),
                users: self.users.iter().collect(),
                assistants: self.assistants.iter().collect(),
                model_requests: self.model_requests.iter().collect(),
                tool_uses: self.tool_uses.iter().collect(),
                tool_outcomes: self.tool_outcomes.iter().collect(),
                tools: self.tools.iter().collect(),
                failures: self.failures.iter().collect(),
                cancellations: self.cancellations.iter().collect(),
                interruptions: self.interruptions.iter().collect(),
                recoveries: self.recoveries.iter().collect(),
                persistence_failures: self.persistence_failures.iter().collect(),
            },
        )
    }
}

pub fn project_transcript(world: &mut World, camera: Entity) -> Vec<TranscriptRow> {
    let camera = *world
        .get::<TranscriptCamera>(camera)
        .expect("transcript camera entity");
    let mut turns = world.query::<(Entity, &Turn)>();
    let mut users = world.query::<&UserMessage>();
    let mut assistants = world.query::<&AssistantMessage>();
    let mut model_requests = world.query::<&ModelRequest>();
    let mut tool_uses = world.query::<(Entity, &ToolUse)>();
    let mut tool_outcomes = world.query::<&ToolOutcome>();
    let mut tools = world.query::<(Entity, &ToolDefinition)>();
    let mut failures = world.query::<&TurnFailed>();
    let mut cancellations = world.query::<&TurnCancelled>();
    let mut interruptions = world.query::<&TurnInterrupted>();
    let mut recoveries = world.query::<&Recovery>();
    let mut persistence_failures = world.query::<&PersistenceFailure>();

    project_transcript_parts(
        camera,
        TranscriptFacts {
            turns: turns.iter(world).collect(),
            users: users.iter(world).collect(),
            assistants: assistants.iter(world).collect(),
            model_requests: model_requests.iter(world).collect(),
            tool_uses: tool_uses.iter(world).collect(),
            tool_outcomes: tool_outcomes.iter(world).collect(),
            tools: tools.iter(world).collect(),
            failures: failures.iter(world).collect(),
            cancellations: cancellations.iter(world).collect(),
            interruptions: interruptions.iter(world).collect(),
            recoveries: recoveries.iter(world).collect(),
            persistence_failures: persistence_failures.iter(world).collect(),
        },
    )
}

pub fn project_context(
    world: &mut World,
    camera: Entity,
) -> Result<ContextDocument, ProjectionError> {
    let camera = *world
        .get::<ContextCamera>(camera)
        .ok_or(ProjectionError::MissingCamera)?;
    project_context_for(world, camera)
}

pub(crate) fn project_context_for(
    world: &mut World,
    camera: ContextCamera,
) -> Result<ContextDocument, ProjectionError> {
    let mut turns = world.query::<(Entity, &Turn)>();
    let mut users = world.query::<&UserMessage>();
    let mut assistants = world.query::<&AssistantMessage>();
    let mut tool_uses = world.query::<(Entity, &ToolUse)>();
    let mut tool_outcomes = world.query::<&ToolOutcome>();
    let mut tool_definitions = world.query::<(Entity, &ToolDefinition)>();
    let mut model_changes = world.query::<&ModelChange>();
    let mut compactions = world.query::<&Compaction>();
    let mut recoveries = world.query::<&Recovery>();
    let mut models = world.query::<(Entity, &Model)>();
    let turns: HashMap<_, _> = turns.iter(world).collect();
    let branch = branch_order(&turns, camera.session, camera.head)?;
    let models: HashMap<_, _> = models
        .iter(world)
        .map(|(entity, model)| (entity, model.model_id.clone()))
        .collect();
    let tools: HashMap<_, _> = tool_definitions
        .iter(world)
        .map(|(entity, tool)| (entity, tool.name.clone()))
        .collect();
    let uses: HashMap<_, _> = tool_uses.iter(world).collect();
    let mut entries = Vec::new();

    entries.extend(users.iter(world).filter_map(|message| {
        branch.get(&message.turn).map(|order| {
            (
                *order,
                message.sequence,
                message.id.0,
                0,
                ContextEntry::User(message.text.clone()),
            )
        })
    }));
    entries.extend(assistants.iter(world).filter_map(|message| {
        branch.get(&message.turn).map(|order| {
            (
                *order,
                message.sequence,
                message.id.0,
                1,
                ContextEntry::Assistant(message.text.clone()),
            )
        })
    }));
    entries.extend(uses.values().filter_map(|tool_use| {
        branch.get(&tool_use.turn).map(|order| {
            (
                *order,
                tool_use.sequence,
                tool_use.id.0,
                2,
                ContextEntry::ToolUse {
                    tool_call_id: tool_use.id,
                    tool: tools
                        .get(&tool_use.tool)
                        .cloned()
                        .unwrap_or_else(|| "unknown tool".into()),
                    input: tool_use.input.clone(),
                },
            )
        })
    }));
    entries.extend(tool_outcomes.iter(world).filter_map(|outcome| {
        let tool_use = uses.get(&outcome.tool_use)?;
        branch.get(&outcome.turn).map(|order| {
            (
                *order,
                outcome.sequence,
                tool_use.id.0,
                3,
                ContextEntry::ToolOutcome {
                    tool_call_id: outcome.tool_call_id,
                    tool: tools
                        .get(&tool_use.tool)
                        .cloned()
                        .unwrap_or_else(|| "unknown tool".into()),
                    output: outcome.output.clone(),
                },
            )
        })
    }));
    for change in model_changes
        .iter(world)
        .filter(|change| branch.contains_key(&change.turn))
    {
        let model_id = models
            .get(&change.model)
            .ok_or(ProjectionError::MissingModel)?;
        entries.push((
            branch[&change.turn],
            change.sequence,
            0,
            4,
            ContextEntry::ModelChange(model_id.clone()),
        ));
    }
    entries.extend(compactions.iter(world).filter_map(|compaction| {
        branch.get(&compaction.turn).map(|order| {
            (
                *order,
                compaction.sequence,
                0,
                5,
                ContextEntry::Compaction(compaction.summary.clone()),
            )
        })
    }));
    entries.extend(recoveries.iter(world).filter_map(|recovery| {
        branch.get(&recovery.turn).map(|order| {
            (
                *order,
                recovery.sequence,
                0,
                6,
                ContextEntry::Recovery(recovery.text.clone()),
            )
        })
    }));
    entries.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
            .then(left.3.cmp(&right.3))
            .then_with(|| context_entry_text(&left.4).cmp(context_entry_text(&right.4)))
    });

    let mut remaining = camera.budget;
    let mut selected = Vec::new();
    for (_, _, _, _, entry) in entries.into_iter().rev() {
        let size = context_entry_text(&entry).len();
        if size <= remaining {
            remaining -= size;
            selected.push(entry);
        }
    }
    selected.reverse();
    Ok(ContextDocument { entries: selected })
}

fn context_entry_text(entry: &ContextEntry) -> &str {
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

fn turn_failure_text(failure: &TurnFailure) -> String {
    match failure {
        TurnFailure::Provider(failure) => format!("Provider: {}", failure.message),
        TurnFailure::Tool(failure) => format!("Tool: {}", failure.message),
        TurnFailure::Recovery(failure) => format!("Recovery: {}", failure.message),
    }
}

fn persistence_failure_rows(
    mut failures: Vec<&PersistenceFailure>,
) -> impl Iterator<Item = TranscriptRow> {
    failures.sort_by_key(|failure| failure.sequence);
    failures
        .into_iter()
        .map(|failure| TranscriptRow::Error(format!("Persistence: {}", failure.message)))
}

fn project_transcript_parts(
    camera: TranscriptCamera,
    facts: TranscriptFacts<'_>,
) -> Vec<TranscriptRow> {
    let TranscriptFacts {
        turns,
        users,
        assistants,
        model_requests,
        tool_uses,
        tool_outcomes,
        tools,
        failures,
        cancellations,
        interruptions,
        recoveries,
        persistence_failures,
    } = facts;
    let turns: HashMap<_, _> = turns.into_iter().collect();
    let branch = match branch_order(&turns, camera.session, camera.head) {
        Ok(branch) => branch,
        Err(error) => {
            let mut rows = vec![TranscriptRow::Error(transcript_error(error).into())];
            rows.extend(persistence_failure_rows(persistence_failures));
            return rows;
        }
    };
    let tools: HashMap<_, _> = tools
        .into_iter()
        .map(|(entity, tool)| (entity, tool.name.clone()))
        .collect();
    let tool_uses: HashMap<_, _> = tool_uses.into_iter().collect();
    let mut items = Vec::new();

    items.extend(users.into_iter().filter_map(|message| {
        branch.get(&message.turn).map(|order| {
            (
                *order,
                message.sequence,
                message.id.0,
                0,
                TranscriptRow::User(message.text.clone()),
            )
        })
    }));
    items.extend(assistants.into_iter().filter_map(|message| {
        branch.get(&message.turn).map(|order| {
            (
                *order,
                message.sequence,
                message.id.0,
                1,
                TranscriptRow::Assistant(message.text.clone()),
            )
        })
    }));
    items.extend(model_requests.into_iter().filter_map(|request| {
        if !matches!(request.status, WorkStatus::Pending | WorkStatus::Running) {
            return None;
        }
        branch.get(&request.turn).map(|order| {
            (
                *order,
                request.sequence,
                request.id.0,
                2,
                TranscriptRow::Work {
                    work: TranscriptWork::Model,
                    status: request.status,
                },
            )
        })
    }));
    items.extend(tool_uses.values().filter_map(|tool_use| {
        branch.get(&tool_use.turn).map(|order| {
            (
                *order,
                tool_use.sequence,
                tool_use.id.0,
                3,
                TranscriptRow::ToolUse {
                    tool: tools
                        .get(&tool_use.tool)
                        .cloned()
                        .unwrap_or_else(|| "unknown tool".into()),
                    input: tool_use.input.clone(),
                },
            )
        })
    }));
    items.extend(tool_uses.values().filter_map(|tool_use| {
        if !matches!(tool_use.status, WorkStatus::Pending | WorkStatus::Running) {
            return None;
        }
        branch.get(&tool_use.turn).map(|order| {
            (
                *order,
                tool_use.sequence,
                tool_use.id.0,
                4,
                TranscriptRow::Work {
                    work: TranscriptWork::Tool(
                        tools
                            .get(&tool_use.tool)
                            .cloned()
                            .unwrap_or_else(|| "unknown tool".into()),
                    ),
                    status: tool_use.status,
                },
            )
        })
    }));
    items.extend(tool_outcomes.into_iter().filter_map(|outcome| {
        let tool_use = tool_uses.get(&outcome.tool_use)?;
        branch.get(&outcome.turn).map(|order| {
            (
                *order,
                outcome.sequence,
                tool_use.id.0,
                5,
                TranscriptRow::ToolOutcome {
                    tool: tools
                        .get(&tool_use.tool)
                        .cloned()
                        .unwrap_or_else(|| "unknown tool".into()),
                    output: outcome.output.clone(),
                },
            )
        })
    }));
    items.extend(failures.into_iter().filter_map(|failure| {
        branch.get(&failure.turn).map(|order| {
            (
                *order,
                failure.sequence,
                failure.generation,
                6,
                TranscriptRow::Error(turn_failure_text(&failure.failure)),
            )
        })
    }));
    items.extend(cancellations.into_iter().filter_map(|outcome| {
        branch.get(&outcome.turn).map(|order| {
            (
                *order,
                outcome.sequence,
                outcome.generation,
                7,
                TranscriptRow::System("Turn cancelled.".into()),
            )
        })
    }));
    items.extend(interruptions.into_iter().filter_map(|outcome| {
        branch.get(&outcome.turn).map(|order| {
            (
                *order,
                outcome.sequence,
                outcome.generation,
                8,
                TranscriptRow::System("Turn interrupted during recovery.".into()),
            )
        })
    }));
    items.extend(recoveries.into_iter().filter_map(|recovery| {
        branch.get(&recovery.turn).map(|order| {
            (
                *order,
                recovery.sequence,
                0,
                9,
                TranscriptRow::System(recovery.text.clone()),
            )
        })
    }));
    items.sort_by_key(|(order, Sequence(sequence), id, kind, _)| (*order, *sequence, *id, *kind));
    let mut rows: Vec<_> = items.into_iter().map(|(_, _, _, _, item)| item).collect();
    rows.extend(persistence_failure_rows(persistence_failures));
    rows
}

fn branch_order(
    turns: &HashMap<Entity, &Turn>,
    session: Entity,
    head: Option<Entity>,
) -> Result<HashMap<Entity, usize>, ProjectionError> {
    let mut branch = Vec::new();
    let mut seen = HashSet::new();
    let mut current = head;

    while let Some(entity) = current {
        let turn = turns.get(&entity).ok_or(ProjectionError::MissingTurn)?;
        if turn.session != session {
            return Err(ProjectionError::CrossSession);
        }
        if !seen.insert(entity) {
            return Err(ProjectionError::Cycle);
        }
        branch.push(entity);
        current = turn.parent;
    }

    Ok(branch
        .into_iter()
        .rev()
        .enumerate()
        .map(|(order, entity)| (entity, order))
        .collect())
}

fn transcript_error(error: ProjectionError) -> &'static str {
    match error {
        ProjectionError::MissingCamera => "Transcript camera does not exist.",
        ProjectionError::MissingTurn => "Transcript branch contains a missing Turn.",
        ProjectionError::CrossSession => "Transcript branch crosses into another Session.",
        ProjectionError::Cycle => "Transcript branch contains a cycle.",
        ProjectionError::MissingModel => "Transcript branch references a missing Model.",
    }
}
