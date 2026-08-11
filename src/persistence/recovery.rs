use std::collections::HashSet;

use bevy::prelude::*;

use super::PersistenceState;
use crate::execution;
use crate::harness::HarnessIds;
use crate::{
    ActiveAgent, ActiveSession, ContextCamera, HarnessReady, ModelRequest, PersistentContextCamera,
    Recovery, Session, SessionId, ToolUse, Turn, TurnInterrupted, WorkStatus,
};

pub(super) fn recover(world: &mut World) {
    let agent = execution::reconcile_fake_capabilities(world);
    ensure_harness_ids(world);
    let session = choose_session(world);
    ensure_context_camera(world, agent, session);
    world.insert_resource(ActiveAgent(agent));
    world.insert_resource(ActiveSession(session));
    execution::sync_persistent_context_camera(
        world,
        agent,
        session,
        world
            .get::<Session>(session)
            .and_then(|value| value.active_head),
    );
    if let Some(message) = world
        .resource_mut::<PersistenceState>()
        .pending_failure
        .take()
    {
        super::record_failure(world, message);
    }
    recover_orphaned_work(world);
    world.insert_resource(HarnessReady);
}

fn ensure_harness_ids(world: &mut World) {
    if !world.contains_resource::<HarnessIds>() {
        world.insert_resource(HarnessIds::default());
    }
}

fn choose_session(world: &mut World) -> Entity {
    world
        .query::<(Entity, &Session)>()
        .iter(world)
        .min_by_key(|(_, session)| session.id.0)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| {
            world
                .spawn(Session {
                    id: SessionId(1),
                    active_head: None,
                })
                .id()
        })
}

fn ensure_context_camera(world: &mut World, agent: Entity, session: Entity) {
    let cameras: Vec<_> = world
        .query::<(Entity, &ContextCamera, &PersistentContextCamera)>()
        .iter(world)
        .map(|(entity, camera, _)| (entity, *camera))
        .collect();
    let head = world
        .get::<Session>(session)
        .and_then(|value| value.active_head);
    let camera = cameras
        .iter()
        .find(|(_, camera)| camera.agent == agent && camera.session == session)
        .map(|(entity, camera)| (*entity, camera.budget));
    if let Some((entity, budget)) = camera {
        world.entity_mut(entity).insert(ContextCamera {
            agent,
            session,
            head,
            budget,
        });
    } else {
        world.spawn((
            ContextCamera {
                agent,
                session,
                head,
                budget: 4096,
            },
            PersistentContextCamera,
        ));
    }
}

fn recover_orphaned_work(world: &mut World) {
    let mut turns = HashSet::new();
    let requests: Vec<_> = world
        .query::<(Entity, &ModelRequest)>()
        .iter(world)
        .filter(|(_, request)| matches!(request.status, WorkStatus::Pending | WorkStatus::Running))
        .map(|(entity, request)| (entity, request.turn))
        .collect();
    for (entity, turn) in requests {
        world
            .get_mut::<ModelRequest>(entity)
            .expect("model work")
            .status = WorkStatus::Cancelled;
        turns.insert(turn);
    }
    let tools: Vec<_> = world
        .query::<(Entity, &ToolUse)>()
        .iter(world)
        .filter(|(_, tool_use)| {
            matches!(tool_use.status, WorkStatus::Pending | WorkStatus::Running)
        })
        .map(|(entity, tool_use)| (entity, tool_use.turn))
        .collect();
    for (entity, turn) in tools {
        world.get_mut::<ToolUse>(entity).expect("tool work").status = WorkStatus::Cancelled;
        turns.insert(turn);
    }
    let mut turns: Vec<_> = turns.into_iter().collect();
    turns.sort_by_key(|entity| entity.to_bits());
    for turn in turns {
        let Some(generation) = world.get_mut::<Turn>(turn).map(|mut value| {
            value.generation += 1;
            value.generation
        }) else {
            continue;
        };
        let (interruption, recovery) = {
            let mut ids = world.resource_mut::<HarnessIds>();
            (ids.sequence(), ids.sequence())
        };
        world.spawn(TurnInterrupted {
            turn,
            generation,
            sequence: interruption,
        });
        world.spawn(Recovery {
            turn,
            text: "Recovered interrupted work after restart.".into(),
            sequence: recovery,
        });
    }
}
