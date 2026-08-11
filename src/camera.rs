use std::collections::{HashMap, HashSet};

use bevy::{ecs::system::SystemParam, prelude::*};

use crate::harness::{
    AssistantMessage, Compaction, Model, ModelChange, Recovery, Sequence, ToolDefinition,
    ToolOutcome, ToolUse, Turn, UserMessage,
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
pub enum TranscriptRow {
    User(String),
    Assistant(String),
    ToolUse { tool: String, input: String },
    ToolOutcome { tool: String, output: String },
    Error(String),
}

#[derive(Component, Debug, Clone, Copy)]
pub struct ContextCamera {
    pub agent: Entity,
    pub session: Entity,
    pub head: Option<Entity>,
    pub budget: usize,
}

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
    tool_uses: Query<'w, 's, (Entity, &'static ToolUse)>,
    tool_outcomes: Query<'w, 's, &'static ToolOutcome>,
    tools: Query<'w, 's, (Entity, &'static ToolDefinition)>,
}

impl TranscriptProjector<'_, '_> {
    pub(crate) fn project(&self, camera: &TranscriptCamera) -> Vec<TranscriptRow> {
        project_transcript_parts(
            *camera,
            self.turns.iter(),
            self.users.iter(),
            self.assistants.iter(),
            self.tool_uses.iter(),
            self.tool_outcomes.iter(),
            self.tools.iter(),
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
    let mut tool_uses = world.query::<(Entity, &ToolUse)>();
    let mut tool_outcomes = world.query::<&ToolOutcome>();
    let mut tools = world.query::<(Entity, &ToolDefinition)>();

    project_transcript_parts(
        camera,
        turns.iter(world),
        users.iter(world),
        assistants.iter(world),
        tool_uses.iter(world),
        tool_outcomes.iter(world),
        tools.iter(world),
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

    Ok(ContextDocument {
        entries: entries
            .into_iter()
            .map(|(_, _, _, _, entry)| entry)
            .collect(),
    })
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

fn project_transcript_parts<'a>(
    camera: TranscriptCamera,
    turns: impl Iterator<Item = (Entity, &'a Turn)>,
    users: impl Iterator<Item = &'a UserMessage>,
    assistants: impl Iterator<Item = &'a AssistantMessage>,
    tool_uses: impl Iterator<Item = (Entity, &'a ToolUse)>,
    tool_outcomes: impl Iterator<Item = &'a ToolOutcome>,
    tools: impl Iterator<Item = (Entity, &'a ToolDefinition)>,
) -> Vec<TranscriptRow> {
    let turns: HashMap<_, _> = turns.collect();
    let branch = match branch_order(&turns, camera.session, camera.head) {
        Ok(branch) => branch,
        Err(error) => return vec![TranscriptRow::Error(transcript_error(error).into())],
    };
    let tools: HashMap<_, _> = tools
        .map(|(entity, tool)| (entity, tool.name.clone()))
        .collect();
    let tool_uses: HashMap<_, _> = tool_uses.collect();
    let mut items = Vec::new();

    items.extend(users.filter_map(|message| {
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
    items.extend(assistants.filter_map(|message| {
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
    items.extend(tool_uses.values().filter_map(|tool_use| {
        branch.get(&tool_use.turn).map(|order| {
            (
                *order,
                tool_use.sequence,
                tool_use.id.0,
                2,
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
    items.extend(tool_outcomes.filter_map(|outcome| {
        let tool_use = tool_uses.get(&outcome.tool_use)?;
        branch.get(&outcome.turn).map(|order| {
            (
                *order,
                outcome.sequence,
                tool_use.id.0,
                3,
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
    items.sort_by_key(|(order, Sequence(sequence), id, kind, _)| (*order, *sequence, *id, *kind));

    items.into_iter().map(|(_, _, _, _, item)| item).collect()
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
