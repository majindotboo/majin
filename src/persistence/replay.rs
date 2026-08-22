use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use bevy::prelude::*;

use super::{
    PersistenceConfig, PersistenceState,
    events::{Event, Record, SCHEMA_VERSION, StoredTurnFailureKind},
    log,
};
use crate::{
    Agent, AssistantMessage, BranchSelection, Compaction, ContextCamera, Model, ModelChange,
    ModelRequest, ModelRequestId, ModelResponse, ModelUsage, PersistentContextCamera, Provider,
    ProviderId, Recovery, Session, SessionId, ToolCallId, ToolDefinition, ToolId, ToolOutcome,
    ToolUse, Turn, TurnCancelled, TurnCompleted, TurnFailed, TurnFailure, TurnId, TurnInterrupted,
    UserMessage,
};

struct LoadedFile {
    session: SessionId,
    path: PathBuf,
    records: Vec<Record>,
}

#[derive(Default)]
struct ReplayState {
    sessions: HashMap<SessionId, Entity>,
    turns: HashMap<TurnId, Entity>,
    user_messages: HashMap<u64, Entity>,
    assistant_messages: HashMap<u64, Entity>,
    requests: HashMap<ModelRequestId, Entity>,
    responses: HashMap<String, Entity>,
    tool_uses: HashMap<ToolCallId, Entity>,
    context_cameras: HashMap<SessionId, Entity>,
    pending_heads: HashMap<SessionId, Option<TurnId>>,
}

pub(super) fn hydrate(world: &mut World) {
    let config = world.resource::<PersistenceConfig>().clone();
    if !config.enabled {
        return;
    }
    let files = match load_files(&config.path) {
        Ok(files) => files,
        Err(error) => return load_failure(world, error),
    };
    if files.is_empty() {
        return;
    }
    if let Err(error) = validate_files(&files, world) {
        return load_failure(world, error);
    }

    let mut replay = ReplayState::default();
    for file in &files {
        let session = world
            .spawn(Session {
                id: file.session,
                active_head: None,
            })
            .id();
        replay.sessions.insert(file.session, session);
        for record in &file.records {
            if let Err(error) = apply_record(world, &mut replay, file.session, record) {
                return load_failure(world, error);
            }
        }
    }
    for (session_id, head) in replay.pending_heads {
        let Some(session) = replay.sessions.get(&session_id).copied() else {
            continue;
        };
        let head = head.and_then(|turn| replay.turns.get(&turn).copied());
        world
            .get_mut::<Session>(session)
            .expect("replayed session")
            .active_head = head;
    }
    initialize_harness_ids(world);
    let next_ordinals: HashMap<_, _> = files
        .iter()
        .map(|file| {
            (
                file.session,
                file.records
                    .iter()
                    .map(|record| record.ordinal)
                    .max()
                    .map_or(0, |ordinal| ordinal + 1),
            )
        })
        .collect();
    if let Err(error) = log::initialize_baseline(world, &next_ordinals) {
        load_failure(world, error);
    }
}

fn initialize_harness_ids(world: &mut World) {
    if !world.contains_resource::<crate::harness::HarnessIds>() {
        world.insert_resource(crate::harness::HarnessIds::default());
    }
    let max_turn = world
        .query::<&Turn>()
        .iter(world)
        .map(|turn| turn.id.0)
        .max()
        .unwrap_or(0);
    let max_message = world
        .query::<&UserMessage>()
        .iter(world)
        .map(|message| message.id.0)
        .max()
        .unwrap_or(0)
        .max(
            world
                .query::<&AssistantMessage>()
                .iter(world)
                .map(|message| message.id.0)
                .max()
                .unwrap_or(0),
        );
    let max_request = world
        .query::<&ModelRequest>()
        .iter(world)
        .map(|request| request.id.0)
        .max()
        .unwrap_or(0);
    let max_tool_call = world
        .query::<&ToolUse>()
        .iter(world)
        .map(|tool_use| tool_use.id.0)
        .max()
        .unwrap_or(0)
        .max(
            world
                .query::<&ToolOutcome>()
                .iter(world)
                .map(|outcome| outcome.tool_call_id.0)
                .max()
                .unwrap_or(0),
        );
    let mut max_sequence = 0;
    macro_rules! max_sequence {
        ($type:ty) => {
            max_sequence = max_sequence.max(
                world
                    .query::<&$type>()
                    .iter(world)
                    .map(|value| value.sequence.0)
                    .max()
                    .unwrap_or(0),
            );
        };
    }
    max_sequence!(Turn);
    max_sequence!(UserMessage);
    max_sequence!(AssistantMessage);
    max_sequence!(ModelRequest);
    max_sequence!(ModelResponse);
    max_sequence!(ToolUse);
    max_sequence!(ToolOutcome);
    max_sequence!(TurnCompleted);
    max_sequence!(TurnCancelled);
    max_sequence!(TurnInterrupted);
    max_sequence!(TurnFailed);
    max_sequence!(BranchSelection);
    max_sequence!(ModelChange);
    max_sequence!(Compaction);
    max_sequence!(Recovery);
    let mut ids = world.resource_mut::<crate::harness::HarnessIds>();
    ids.restore_from_maxima(
        max_turn,
        max_message,
        max_request,
        max_tool_call,
        max_sequence,
    );
}

fn load_files(root: &Path) -> Result<Vec<LoadedFile>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(root)
        .map_err(|error| format!("Persistence directory read failed: {error}"))?;
    let mut paths: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("jsonl"))
        .collect();
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        let session = parse_session_path(&path)?;
        let records = read_records(&path)?;
        if records.is_empty() {
            return Err(format!("Persistence log is empty: {}", path.display()));
        }
        files.push(LoadedFile {
            session,
            path,
            records,
        });
    }
    Ok(files)
}

fn parse_session_path(path: &Path) -> Result<SessionId, String> {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            format!(
                "Persistence log has an invalid filename: {}",
                path.display()
            )
        })?;
    let value = stem
        .strip_prefix("session-")
        .ok_or_else(|| {
            format!(
                "Persistence log has an invalid filename: {}",
                path.display()
            )
        })?
        .parse::<u64>()
        .map_err(|error| format!("Persistence log has an invalid session ID: {error}"))?;
    Ok(SessionId(value))
}

fn read_records(path: &Path) -> Result<Vec<Record>, String> {
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "Persistence log read failed for {}: {error}",
            path.display()
        )
    })?;
    let mut records = Vec::new();
    let mut offset = 0;
    for chunk in bytes.split_inclusive(|byte| *byte == b'\n') {
        let complete = chunk.ends_with(b"\n");
        let line = chunk.strip_suffix(b"\n").unwrap_or(chunk);
        if line.iter().all(u8::is_ascii_whitespace) {
            offset += chunk.len();
            continue;
        }
        match serde_json::from_slice::<Record>(line) {
            Ok(record) => records.push(record),
            Err(error) if !complete && error.is_eof() => {
                truncate_partial_line(path, offset)?;
                break;
            }
            Err(error) => {
                return Err(format!(
                    "Persistence log decode failed for {}: {error}",
                    path.display()
                ));
            }
        }
        offset += chunk.len();
    }
    Ok(records)
}

fn truncate_partial_line(path: &Path, length: usize) -> Result<(), String> {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| {
            format!(
                "Persistence log repair failed for {}: {error}",
                path.display()
            )
        })?;
    file.set_len(length as u64).map_err(|error| {
        format!(
            "Persistence log repair failed for {}: {error}",
            path.display()
        )
    })
}

fn validate_files(files: &[LoadedFile], world: &mut World) -> Result<(), String> {
    let mut sessions = HashSet::new();
    let mut turns = HashMap::new();
    let mut requests = HashMap::new();
    let mut tool_uses = HashMap::new();
    let mut messages = HashMap::new();
    let mut responses = HashMap::new();
    for file in files {
        if !sessions.insert(file.session) {
            return Err(format!(
                "Persistence load failed: duplicate Session {:?}.",
                file.session
            ));
        }
        let mut has_session = false;
        let mut previous_ordinal = None;
        for record in &file.records {
            if record.schema != SCHEMA_VERSION {
                return Err(format!(
                    "Persistence load failed: unsupported schema version {} in {}.",
                    record.schema,
                    file.path.display()
                ));
            }
            if previous_ordinal.is_some_and(|previous| record.ordinal <= previous) {
                return Err(format!(
                    "Persistence load failed: non-increasing ordinal in {}.",
                    file.path.display()
                ));
            }
            previous_ordinal = Some(record.ordinal);
            match &record.event {
                Event::Session { id, .. } => {
                    has_session = true;
                    if *id != file.session.0 {
                        return Err(format!(
                            "Persistence load failed: Session record ID does not match {}.",
                            file.path.display()
                        ));
                    }
                }
                Event::Turn { id, .. } => {
                    if turns
                        .insert(*id, file.session)
                        .is_some_and(|existing| existing != file.session)
                    {
                        return Err(format!("Persistence load failed: duplicate TurnId {id}."));
                    }
                }
                Event::ModelRequest { id, .. } => {
                    if requests
                        .insert(*id, file.session)
                        .is_some_and(|existing| existing != file.session)
                    {
                        return Err(format!(
                            "Persistence load failed: duplicate ModelRequestId {id}."
                        ));
                    }
                }
                Event::ToolUse { id, .. } => {
                    if tool_uses
                        .insert(*id, file.session)
                        .is_some_and(|existing| existing != file.session)
                    {
                        return Err(format!(
                            "Persistence load failed: duplicate ToolCallId {id}."
                        ));
                    }
                }
                Event::UserMessage { id, .. } | Event::AssistantMessage { id, .. } => {
                    if messages
                        .insert(*id, file.session)
                        .is_some_and(|existing| existing != file.session)
                    {
                        return Err(format!(
                            "Persistence load failed: duplicate MessageId {id}."
                        ));
                    }
                }
                Event::ModelResponse { response_id, .. }
                    if responses
                        .insert(response_id, file.session)
                        .is_some_and(|existing| existing != file.session) =>
                {
                    return Err(format!(
                        "Persistence load failed: duplicate response ID {response_id:?}."
                    ));
                }
                _ => {}
            }
        }
        if !has_session {
            return Err(format!(
                "Persistence load failed: {} has no Session record.",
                file.path.display()
            ));
        }
    }
    let turn_in = |id: u64, session: SessionId, field: &str| {
        if turns.get(&id) == Some(&session) {
            Ok(())
        } else {
            Err(format!(
                "Persistence load failed: {field} references TurnId {id} outside Session {:?}.",
                session
            ))
        }
    };
    for file in files {
        for record in &file.records {
            match &record.event {
                Event::Session {
                    active_head: Some(head),
                    ..
                } => turn_in(*head, file.session, "Session.active_head")?,
                Event::Turn {
                    parent: Some(parent),
                    ..
                } => turn_in(*parent, file.session, "Turn.parent")?,
                Event::UserMessage { turn, .. }
                | Event::AssistantMessage { turn, .. }
                | Event::ToolUse { turn, .. }
                | Event::TurnCompleted { turn, .. }
                | Event::TurnCancelled { turn, .. }
                | Event::TurnInterrupted { turn, .. }
                | Event::TurnFailed { turn, .. }
                | Event::ModelChange { turn, .. }
                | Event::Compaction { turn, .. }
                | Event::Recovery { turn, .. } => turn_in(*turn, file.session, "event.turn")?,
                Event::BranchSelection { head, .. } => {
                    turn_in(*head, file.session, "BranchSelection.head")?;
                }
                Event::ModelRequest {
                    turn,
                    previous_tool_use,
                    ..
                } => {
                    turn_in(*turn, file.session, "event.turn")?;
                    if let Some(tool_use) = previous_tool_use
                        && !tool_uses.contains_key(tool_use)
                    {
                        return Err(format!(
                            "Persistence load failed: missing ToolCallId {tool_use}."
                        ));
                    }
                }
                Event::ModelResponse { turn, request, .. } => {
                    turn_in(*turn, file.session, "event.turn")?;
                    if !requests.contains_key(request) {
                        return Err(format!(
                            "Persistence load failed: missing ModelRequestId {request}."
                        ));
                    }
                }
                Event::ToolOutcome {
                    turn,
                    tool_use,
                    tool_call_id,
                    ..
                } => {
                    turn_in(*turn, file.session, "event.turn")?;
                    if !tool_uses.contains_key(tool_use) || tool_use != tool_call_id {
                        return Err(format!(
                            "Persistence load failed: invalid ToolOutcome reference {tool_use}."
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    validate_capabilities(files, world)
}

fn validate_capabilities(files: &[LoadedFile], world: &mut World) -> Result<(), String> {
    for file in files {
        for record in &file.records {
            let (model_id, provider_id) = match &record.event {
                Event::ModelRequest {
                    model_id,
                    provider_id,
                    ..
                }
                | Event::ModelResponse {
                    model_id,
                    provider_id,
                    ..
                }
                | Event::ToolUse {
                    model_id,
                    provider_id,
                    ..
                }
                | Event::ModelChange {
                    model_id,
                    provider_id,
                    ..
                } => (model_id, *provider_id),
                _ => continue,
            };
            let provider = world
                .query::<(Entity, &Provider)>()
                .iter(world)
                .find(|(_, value)| value.provider_id.0 == provider_id)
                .map(|(entity, _)| entity);
            let Some(provider) = provider else {
                return Err(format!(
                    "Persistence load failed: missing ProviderId {provider_id}."
                ));
            };
            if !world
                .query::<&Model>()
                .iter(world)
                .any(|model| model.provider == provider && model.model_id == *model_id)
            {
                return Err(format!(
                    "Persistence load failed: missing model {model_id:?} for ProviderId {provider_id}."
                ));
            }
            if let Event::ToolUse { tool_id, .. } = &record.event
                && !world
                    .query::<&ToolDefinition>()
                    .iter(world)
                    .any(|tool| tool.tool_id.0 == *tool_id)
            {
                return Err(format!(
                    "Persistence load failed: missing ToolId {tool_id}."
                ));
            }
        }
    }
    Ok(())
}

fn apply_record(
    world: &mut World,
    replay: &mut ReplayState,
    session_id: SessionId,
    record: &Record,
) -> Result<(), String> {
    let session = *replay
        .sessions
        .get(&session_id)
        .ok_or_else(|| format!("missing replay Session {:?}", session_id))?;
    match &record.event {
        Event::Session { active_head, .. } => {
            replay
                .pending_heads
                .insert(session_id, active_head.map(TurnId));
        }
        Event::ContextCamera { budget } => {
            let agent = first_agent(world)?;
            let head = world
                .get::<Session>(session)
                .and_then(|value| value.active_head);
            let camera = replay.context_cameras.get(&session_id).copied();
            let entity = camera.unwrap_or_else(|| {
                world
                    .spawn((
                        ContextCamera {
                            agent,
                            session,
                            head,
                            budget: *budget,
                        },
                        PersistentContextCamera,
                    ))
                    .id()
            });
            if camera.is_some() {
                world.entity_mut(entity).insert(ContextCamera {
                    agent,
                    session,
                    head,
                    budget: *budget,
                });
            }
            replay.context_cameras.insert(session_id, entity);
        }
        Event::Turn {
            id,
            parent,
            sequence,
            generation,
        } => {
            let parent = parent
                .map(|parent| {
                    replay.turns.get(&TurnId(parent)).copied().ok_or_else(|| {
                        format!("missing replay parent TurnId {parent} for TurnId {id}")
                    })
                })
                .transpose()?;
            let turn = if let Some(entity) = replay.turns.get(&TurnId(*id)).copied() {
                entity
            } else {
                let entity = world
                    .spawn(Turn {
                        id: TurnId(*id),
                        session,
                        parent,
                        sequence: crate::Sequence(*sequence),
                        generation: *generation,
                    })
                    .id();
                replay.turns.insert(TurnId(*id), entity);
                entity
            };
            world.entity_mut(turn).insert(Turn {
                id: TurnId(*id),
                session,
                parent,
                sequence: crate::Sequence(*sequence),
                generation: *generation,
            });
        }
        Event::UserMessage {
            id,
            turn,
            sequence,
            text,
        } => {
            let turn = entity_for_turn(replay, *turn)?;
            let entity = replay.user_messages.get(id).copied().unwrap_or_else(|| {
                let entity = world.spawn_empty().id();
                replay.user_messages.insert(*id, entity);
                entity
            });
            world.entity_mut(entity).insert(UserMessage {
                id: crate::MessageId(*id),
                turn,
                sequence: crate::Sequence(*sequence),
                text: text.clone(),
            });
        }
        Event::AssistantMessage {
            id,
            turn,
            sequence,
            text,
        } => {
            let turn = entity_for_turn(replay, *turn)?;
            let entity = replay
                .assistant_messages
                .get(id)
                .copied()
                .unwrap_or_else(|| {
                    let entity = world.spawn_empty().id();
                    replay.assistant_messages.insert(*id, entity);
                    entity
                });
            world.entity_mut(entity).insert(AssistantMessage {
                id: crate::MessageId(*id),
                turn,
                sequence: crate::Sequence(*sequence),
                text: text.clone(),
            });
        }
        Event::ModelRequest {
            id,
            turn,
            model_id,
            provider_id,
            generation,
            previous_tool_use,
            status,
            sequence,
        } => {
            let turn_entity = entity_for_turn(replay, *turn)?;
            let model = find_model(world, model_id, *provider_id)?;
            let provider = world.get::<Model>(model).expect("model").provider;
            let agent = find_agent(world, model)?;
            let previous_tool_use = previous_tool_use
                .map(|id| entity_for_tool_use(replay, id))
                .transpose()?;
            let entity = replay
                .requests
                .get(&ModelRequestId(*id))
                .copied()
                .unwrap_or_else(|| {
                    let entity = world.spawn_empty().id();
                    replay.requests.insert(ModelRequestId(*id), entity);
                    entity
                });
            world.entity_mut(entity).insert(ModelRequest {
                id: ModelRequestId(*id),
                turn: turn_entity,
                agent,
                model,
                provider,
                generation: *generation,
                previous_tool_use,
                status: (*status).into(),
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::ModelResponse {
            request,
            turn,
            model_id,
            provider_id,
            generation,
            response_id,
            api,
            input_tokens,
            output_tokens,
            stop_reason,
            opaque_replay,
            sequence,
        } => {
            let request_entity = replay
                .requests
                .get(&ModelRequestId(*request))
                .copied()
                .ok_or_else(|| format!("missing replay ModelRequestId {request}"))?;
            let turn = entity_for_turn(replay, *turn)?;
            let model = find_model(world, model_id, *provider_id)?;
            let provider = world.get::<Model>(model).expect("model").provider;
            let entity = replay
                .responses
                .get(response_id)
                .copied()
                .unwrap_or_else(|| {
                    let entity = world.spawn_empty().id();
                    replay.responses.insert(response_id.clone(), entity);
                    entity
                });
            world.entity_mut(entity).insert(ModelResponse {
                request: request_entity,
                turn,
                model,
                generation: *generation,
                provider,
                response_id: response_id.clone(),
                api: (*api).into(),
                usage: ModelUsage {
                    input_tokens: *input_tokens,
                    output_tokens: *output_tokens,
                },
                stop_reason: (*stop_reason).into(),
                opaque_replay: opaque_replay.clone(),
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::ToolUse {
            id,
            turn,
            tool_id,
            model_id,
            provider_id,
            generation,
            input,
            status,
            sequence,
        } => {
            let turn = entity_for_turn(replay, *turn)?;
            let model = find_model(world, model_id, *provider_id)?;
            let provider = world.get::<Model>(model).expect("model").provider;
            let agent = find_agent(world, model)?;
            let tool = find_tool(world, *tool_id)?;
            let entity = replay
                .tool_uses
                .get(&ToolCallId(*id))
                .copied()
                .unwrap_or_else(|| {
                    let entity = world.spawn_empty().id();
                    replay.tool_uses.insert(ToolCallId(*id), entity);
                    entity
                });
            world.entity_mut(entity).insert(ToolUse {
                id: ToolCallId(*id),
                turn,
                agent,
                tool,
                model,
                provider,
                generation: *generation,
                input: input.clone(),
                status: (*status).into(),
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::ToolOutcome {
            tool_use,
            tool_call_id,
            turn,
            generation,
            output,
            sequence,
        } => {
            let tool_use = entity_for_tool_use(replay, *tool_use)?;
            let turn = entity_for_turn(replay, *turn)?;
            world.spawn(ToolOutcome {
                tool_use,
                tool_call_id: ToolCallId(*tool_call_id),
                turn,
                generation: *generation,
                output: output.clone(),
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::TurnCompleted {
            turn,
            generation,
            sequence,
        } => {
            world.spawn(TurnCompleted {
                turn: entity_for_turn(replay, *turn)?,
                generation: *generation,
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::TurnCancelled {
            turn,
            generation,
            sequence,
        } => {
            world.spawn(TurnCancelled {
                turn: entity_for_turn(replay, *turn)?,
                generation: *generation,
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::TurnInterrupted {
            turn,
            generation,
            sequence,
        } => {
            world.spawn(TurnInterrupted {
                turn: entity_for_turn(replay, *turn)?,
                generation: *generation,
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::TurnFailed {
            turn,
            generation,
            failure,
            sequence,
        } => {
            world.spawn(TurnFailed {
                turn: entity_for_turn(replay, *turn)?,
                generation: *generation,
                failure: turn_failure(failure),
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::BranchSelection { head, sequence } => {
            let head = entity_for_turn(replay, *head)?;
            world
                .get_mut::<Session>(session)
                .expect("session")
                .active_head = Some(head);
            world.spawn(BranchSelection {
                session,
                head,
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::ModelChange {
            turn,
            model_id,
            provider_id,
            sequence,
        } => {
            let model = find_model(world, model_id, *provider_id)?;
            world.spawn(ModelChange {
                turn: entity_for_turn(replay, *turn)?,
                model,
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::Compaction {
            turn,
            summary,
            sequence,
        } => {
            world.spawn(Compaction {
                turn: entity_for_turn(replay, *turn)?,
                summary: summary.clone(),
                sequence: crate::Sequence(*sequence),
            });
        }
        Event::Recovery {
            turn,
            text,
            sequence,
        } => {
            world.spawn(Recovery {
                turn: entity_for_turn(replay, *turn)?,
                text: text.clone(),
                sequence: crate::Sequence(*sequence),
            });
        }
    };
    Ok(())
}

fn entity_for_turn(replay: &ReplayState, id: u64) -> Result<Entity, String> {
    replay
        .turns
        .get(&TurnId(id))
        .copied()
        .ok_or_else(|| format!("missing replay TurnId {id}"))
}

fn entity_for_tool_use(replay: &ReplayState, id: u64) -> Result<Entity, String> {
    replay
        .tool_uses
        .get(&ToolCallId(id))
        .copied()
        .ok_or_else(|| format!("missing replay ToolCallId {id}"))
}

fn first_agent(world: &mut World) -> Result<Entity, String> {
    world
        .query_filtered::<Entity, With<Agent>>()
        .iter(world)
        .next()
        .ok_or_else(|| "Persistence load failed: no Agent capability registered.".into())
}

fn find_model(world: &mut World, model_id: &str, provider_id: u64) -> Result<Entity, String> {
    let provider = world
        .query::<(Entity, &Provider)>()
        .iter(world)
        .find(|(_, provider)| provider.provider_id == ProviderId(provider_id))
        .map(|(entity, _)| entity)
        .ok_or_else(|| format!("missing ProviderId {provider_id}"))?;
    world
        .query::<(Entity, &Model)>()
        .iter(world)
        .find(|(_, model)| model.provider == provider && model.model_id == model_id)
        .map(|(entity, _)| entity)
        .ok_or_else(|| format!("missing model {model_id:?} for ProviderId {provider_id}"))
}

fn find_agent(world: &mut World, model: Entity) -> Result<Entity, String> {
    world
        .query::<(Entity, &Agent)>()
        .iter(world)
        .find(|(_, agent)| agent.model == model)
        .map(|(entity, _)| entity)
        .ok_or_else(|| "missing Agent for persisted Model".into())
}

fn find_tool(world: &mut World, tool_id: u64) -> Result<Entity, String> {
    world
        .query::<(Entity, &ToolDefinition)>()
        .iter(world)
        .find(|(_, tool)| tool.tool_id == ToolId(tool_id))
        .map(|(entity, _)| entity)
        .ok_or_else(|| format!("missing ToolId {tool_id}"))
}

fn turn_failure(failure: &crate::persistence::events::StoredTurnFailure) -> TurnFailure {
    let message = failure.message.clone();
    match failure.kind {
        StoredTurnFailureKind::Provider => {
            TurnFailure::Provider(crate::ProviderFailure { message })
        }
        StoredTurnFailureKind::Tool => TurnFailure::Tool(crate::ToolFailure { message }),
        StoredTurnFailureKind::Recovery => {
            TurnFailure::Recovery(crate::RecoveryFailure { message })
        }
    }
}

fn load_failure(world: &mut World, message: String) {
    let mut state = world.resource_mut::<PersistenceState>();
    state.blocked = true;
    state.pending_failure = Some(message);
}
