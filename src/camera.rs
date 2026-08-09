use std::collections::HashSet;

use bevy::prelude::*;

use crate::{
    MajinStartupSet,
    harness::{ActiveSession, AssistantMessage, MessageId, Sequence, Turn, UserMessage},
};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            spawn_transcript_camera.in_set(MajinStartupSet::Camera),
        );
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct TranscriptCamera {
    pub session: Entity,
    pub scroll_from_bottom: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptKind {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptItem {
    pub kind: TranscriptKind,
    pub title: &'static str,
    pub body: String,
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

pub(crate) fn project_transcript_parts<'a>(
    camera: TranscriptCamera,
    turns: impl Iterator<Item = (Entity, &'a Turn)>,
    users: impl Iterator<Item = &'a UserMessage>,
    assistants: impl Iterator<Item = &'a AssistantMessage>,
) -> Vec<TranscriptItem> {
    let session_turns: HashSet<_> = turns
        .into_iter()
        .filter_map(|(entity, turn)| (turn.session == camera.session).then_some(entity))
        .collect();
    let mut items = Vec::new();

    items.extend(users.filter_map(|message| {
        session_turns.contains(&message.turn).then_some((
            message.sequence,
            message.id,
            TranscriptItem {
                kind: TranscriptKind::User,
                title: "YOU",
                body: message.text.clone(),
            },
        ))
    }));
    items.extend(assistants.filter_map(|message| {
        session_turns.contains(&message.turn).then_some((
            message.sequence,
            message.id,
            TranscriptItem {
                kind: TranscriptKind::Assistant,
                title: "MAJIN",
                body: message.text.clone(),
            },
        ))
    }));
    items.sort_by_key(|(Sequence(sequence), MessageId(id), _)| (*sequence, *id));

    items.into_iter().map(|(_, _, item)| item).collect()
}

fn spawn_transcript_camera(world: &mut World) {
    let session = world.resource::<ActiveSession>().0;
    world.spawn(TranscriptCamera {
        session,
        scroll_from_bottom: 0,
    });
}
