use std::collections::{HashMap, HashSet};

use bevy::{ecs::system::SystemParam, prelude::*};

use crate::harness::{AssistantMessage, MessageId, Sequence, Turn, UserMessage};

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
    Error(String),
}

#[derive(SystemParam)]
pub(crate) struct TranscriptProjector<'w, 's> {
    turns: Query<'w, 's, (Entity, &'static Turn)>,
    users: Query<'w, 's, &'static UserMessage>,
    assistants: Query<'w, 's, &'static AssistantMessage>,
}

impl TranscriptProjector<'_, '_> {
    pub(crate) fn project(&self, camera: &TranscriptCamera) -> Vec<TranscriptRow> {
        project_transcript_parts(
            *camera,
            self.turns.iter(),
            self.users.iter(),
            self.assistants.iter(),
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

    project_transcript_parts(
        camera,
        turns.iter(world),
        users.iter(world),
        assistants.iter(world),
    )
}

fn project_transcript_parts<'a>(
    camera: TranscriptCamera,
    turns: impl Iterator<Item = (Entity, &'a Turn)>,
    users: impl Iterator<Item = &'a UserMessage>,
    assistants: impl Iterator<Item = &'a AssistantMessage>,
) -> Vec<TranscriptRow> {
    let turns: HashMap<_, _> = turns.collect();
    let mut branch = Vec::new();
    let mut seen = HashSet::new();
    let mut current = camera.head;

    while let Some(entity) = current {
        let Some(turn) = turns.get(&entity) else {
            return vec![TranscriptRow::Error(
                "Transcript branch contains a missing Turn.".into(),
            )];
        };
        if turn.session != camera.session {
            return vec![TranscriptRow::Error(
                "Transcript branch crosses into another Session.".into(),
            )];
        }
        if !seen.insert(entity) {
            return vec![TranscriptRow::Error(
                "Transcript branch contains a cycle.".into(),
            )];
        }
        branch.push(entity);
        current = turn.parent;
    }
    branch.reverse();

    let branch: HashMap<_, _> = branch
        .into_iter()
        .enumerate()
        .map(|(order, entity)| (entity, order))
        .collect();
    let mut items = Vec::new();

    items.extend(users.filter_map(|message| {
        branch.get(&message.turn).map(|order| {
            (
                *order,
                message.sequence,
                message.id,
                TranscriptRow::User(message.text.clone()),
            )
        })
    }));
    items.extend(assistants.filter_map(|message| {
        branch.get(&message.turn).map(|order| {
            (
                *order,
                message.sequence,
                message.id,
                TranscriptRow::Assistant(message.text.clone()),
            )
        })
    }));
    items.sort_by_key(|(order, Sequence(sequence), MessageId(id), _)| (*order, *sequence, *id));

    items.into_iter().map(|(_, _, _, item)| item).collect()
}
