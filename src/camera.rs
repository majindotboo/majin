use std::collections::HashSet;

use bevy::{ecs::system::SystemParam, prelude::*};

use crate::harness::{AssistantMessage, MessageId, Sequence, Turn, UserMessage};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, _app: &mut App) {}
}

#[derive(Component, Debug, Clone, Copy)]
pub struct TranscriptCamera {
    pub session: Entity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptItem {
    User(String),
    Assistant(String),
}

#[derive(SystemParam)]
pub struct TranscriptProjection<'w, 's> {
    turns: Query<'w, 's, (Entity, &'static Turn)>,
    users: Query<'w, 's, &'static UserMessage>,
    assistants: Query<'w, 's, &'static AssistantMessage>,
}

impl TranscriptProjection<'_, '_> {
    pub fn project(&self, camera: &TranscriptCamera) -> Vec<TranscriptItem> {
        project_transcript_parts(
            *camera,
            self.turns.iter(),
            self.users.iter(),
            self.assistants.iter(),
        )
    }
}

pub fn project_transcript(world: &mut World, camera: Entity) -> Vec<TranscriptItem> {
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
) -> Vec<TranscriptItem> {
    let session_turns: HashSet<_> = turns
        .filter_map(|(entity, turn)| (turn.session == camera.session).then_some(entity))
        .collect();
    let mut items = Vec::new();

    items.extend(users.filter_map(|message| {
        session_turns.contains(&message.turn).then_some((
            message.sequence,
            message.id,
            TranscriptItem::User(message.text.clone()),
        ))
    }));
    items.extend(assistants.filter_map(|message| {
        session_turns.contains(&message.turn).then_some((
            message.sequence,
            message.id,
            TranscriptItem::Assistant(message.text.clone()),
        ))
    }));
    items.sort_by_key(|(Sequence(sequence), MessageId(id), _)| (*sequence, *id));

    items.into_iter().map(|(_, _, item)| item).collect()
}
