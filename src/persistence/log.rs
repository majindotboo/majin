use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    time::Instant,
};

use bevy::prelude::*;
use serde_json::Error as JsonError;

use super::{
    PersistenceConfig, PersistenceState,
    events::{Event, Record, SCHEMA_VERSION},
    record_failure, session_path,
};
use crate::{
    AssistantMessage, BranchSelection, Compaction, ContextCamera, ModelChange, ModelRequest,
    ModelResponse, PersistentContextCamera, Provider, Recovery, Session, SessionId, ToolOutcome,
    ToolUse, Turn, TurnCancelled, TurnCompleted, TurnFailed, TurnId, TurnInterrupted, UserMessage,
};

struct EventEntry {
    session: SessionId,
    event: Event,
}

pub(super) fn persist(world: &mut World) {
    let config = world.resource::<PersistenceConfig>().clone();
    if !config.enabled || world.resource::<PersistenceState>().blocked {
        return;
    }
    let current = match collect_events(world) {
        Ok(events) => events,
        Err(error) => {
            record_failure(world, format!("Persistence serialization failed: {error}"));
            return;
        }
    };
    let changed = {
        let state = world.resource::<PersistenceState>();
        current
            .iter()
            .filter(|(key, entry)| {
                let value = serde_json::to_string(&entry.event).expect("event serialization");
                state.last_values.get(*key) != Some(&value)
            })
            .map(|(key, entry)| {
                (
                    key.clone(),
                    entry.session,
                    entry.event.clone(),
                    serde_json::to_string(&entry.event).expect("event serialization"),
                )
            })
            .collect::<Vec<_>>()
    };
    if changed.is_empty() {
        world.resource_mut::<PersistenceState>().dirty_since = None;
        return;
    }
    let should_write = {
        let mut state = world.resource_mut::<PersistenceState>();
        let since = state.dirty_since.get_or_insert_with(Instant::now);
        since.elapsed() >= config.debounce
    };
    let exiting = world
        .get_resource::<Messages<AppExit>>()
        .is_some_and(|messages| !messages.is_empty());
    if !should_write && !exiting {
        return;
    }

    let mut by_session: HashMap<SessionId, Vec<(String, Event, String)>> = HashMap::new();
    for (key, session, event, value) in changed {
        by_session
            .entry(session)
            .or_default()
            .push((key, event, value));
    }
    let mut persisted = Vec::new();
    for (session, mut events) in by_session {
        events.sort_by_key(|(_, event, _)| (event.sequence().unwrap_or(0), event.key()));
        let ordinal = world
            .resource::<PersistenceState>()
            .next_ordinals
            .get(&session)
            .copied()
            .unwrap_or(0);
        let records: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(index, (_, event, _))| Record {
                schema: SCHEMA_VERSION,
                ordinal: ordinal + index as u64,
                event: event.clone(),
            })
            .collect();
        match append_records(&session_path(&config.path, session), &records) {
            Ok(()) => {
                persisted.extend(events.into_iter().map(|(key, _, value)| (key, value)));
                world
                    .resource_mut::<PersistenceState>()
                    .next_ordinals
                    .insert(session, ordinal + records.len() as u64);
            }
            Err(error) => {
                record_failure(world, format!("Persistence save failed: {error}"));
                break;
            }
        }
    }
    let mut state = world.resource_mut::<PersistenceState>();
    for (key, value) in persisted {
        state.last_values.insert(key, value);
    }
    if state.last_values.len() == current.len() {
        state.dirty_since = None;
    } else {
        state.dirty_since = Some(Instant::now());
    }
}

pub(super) fn initialize_baseline(
    world: &mut World,
    next_ordinals: &HashMap<crate::SessionId, u64>,
) -> Result<(), String> {
    let current = collect_events(world)?;
    let mut state = world.resource_mut::<PersistenceState>();
    state.last_values = current
        .into_iter()
        .map(|(key, entry)| {
            let value = serde_json::to_string(&entry.event).expect("event serialization");
            (key, value)
        })
        .collect();
    state.next_ordinals = next_ordinals.clone();
    Ok(())
}

fn append_records(path: &Path, records: &[Record]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut options = OpenOptions::new();
    options.create(true).append(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    if file.metadata()?.len() > 0 {
        let mut previous = File::open(path)?;
        previous.seek(SeekFrom::End(-1))?;
        let mut byte = [0];
        previous.read_exact(&mut byte)?;
        if byte[0] != b'\n' {
            file.write_all(b"\n")?;
        }
    }
    for record in records {
        serde_json::to_writer(&mut file, record).map_err(json_io_error)?;
        file.write_all(b"\n")?;
    }
    file.sync_data()
}

fn json_io_error(error: JsonError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn collect_events(world: &mut World) -> Result<HashMap<String, EventEntry>, String> {
    let sessions: HashMap<_, _> = {
        let mut query = world.query::<(Entity, &Session)>();
        query
            .iter(world)
            .map(|(entity, session)| (entity, (session.id, session.active_head)))
            .collect()
    };
    let turns: HashMap<_, _> = {
        let mut query = world.query::<(Entity, &Turn)>();
        query
            .iter(world)
            .map(|(entity, turn)| (entity, (turn.session, TurnId(turn.id.0))))
            .collect()
    };
    let models: HashMap<_, _> = {
        let mut query = world.query::<(Entity, &crate::Model)>();
        query
            .iter(world)
            .filter_map(|(entity, model)| {
                world
                    .get::<Provider>(model.provider)
                    .map(|provider| (entity, (model.model_id.clone(), provider.provider_id.0)))
            })
            .collect()
    };
    let tools: HashMap<_, _> = {
        let mut query = world.query::<(Entity, &crate::ToolDefinition)>();
        query
            .iter(world)
            .map(|(entity, tool)| (entity, tool.tool_id.0))
            .collect()
    };
    let tool_uses: HashMap<_, _> = {
        let mut query = world.query::<(Entity, &ToolUse)>();
        query
            .iter(world)
            .map(|(entity, tool_use)| (entity, tool_use.id.0))
            .collect()
    };
    let mut events = HashMap::new();
    let mut session_ids = HashSet::new();
    for (session_id, active_head) in sessions.values() {
        if !session_ids.insert(*session_id) {
            return Err(format!("duplicate SessionId {}", session_id.0));
        }
        let active_head = active_head
            .map(|head| {
                turns.get(&head).map(|(_, turn)| turn.0).ok_or_else(|| {
                    format!(
                        "Session {} references missing active head {head:?}",
                        session_id.0
                    )
                })
            })
            .transpose()?;
        insert(
            &mut events,
            *session_id,
            Event::Session {
                id: session_id.0,
                active_head,
            },
        )?;
    }
    for (entity, camera, _) in world
        .query::<(Entity, &ContextCamera, &PersistentContextCamera)>()
        .iter(world)
    {
        let session_id = sessions
            .get(&camera.session)
            .map(|(id, _)| *id)
            .ok_or_else(|| {
                format!("persistent ContextCamera {entity:?} references missing Session")
            })?;
        insert(
            &mut events,
            session_id,
            Event::ContextCamera {
                budget: camera.budget,
            },
        )?;
    }
    for (entity, turn) in world.query::<(Entity, &Turn)>().iter(world) {
        let (session_entity, _) = turns.get(&entity).expect("turn map contains query turn");
        let session_id = sessions
            .get(session_entity)
            .map(|(id, _)| *id)
            .ok_or_else(|| format!("Turn {entity:?} references missing Session"))?;
        let parent = turn
            .parent
            .map(|parent| {
                turns
                    .get(&parent)
                    .map(|(_, parent_turn)| parent_turn.0)
                    .ok_or_else(|| {
                        format!("Turn {} references missing parent {parent:?}", turn.id.0)
                    })
            })
            .transpose()?;
        insert(
            &mut events,
            session_id,
            Event::Turn {
                id: turn.id.0,
                parent,
                sequence: turn.sequence.0,
                generation: turn.generation,
            },
        )?;
    }
    for (_, message) in world.query::<(Entity, &UserMessage)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, message.turn)?;
        insert(
            &mut events,
            session_id,
            Event::UserMessage {
                id: message.id.0,
                turn: turn_id.0,
                sequence: message.sequence.0,
                text: message.text.clone(),
            },
        )?;
    }
    for (_, message) in world.query::<(Entity, &AssistantMessage)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, message.turn)?;
        insert(
            &mut events,
            session_id,
            Event::AssistantMessage {
                id: message.id.0,
                turn: turn_id.0,
                sequence: message.sequence.0,
                text: message.text.clone(),
            },
        )?;
    }
    for (_, request) in world.query::<(Entity, &ModelRequest)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, request.turn)?;
        let (model_id, provider_id) = models
            .get(&request.model)
            .cloned()
            .ok_or_else(|| "ModelRequest references missing Model capability".to_string())?;
        insert(
            &mut events,
            session_id,
            Event::ModelRequest {
                id: request.id.0,
                turn: turn_id.0,
                model_id,
                provider_id,
                generation: request.generation,
                previous_tool_use: request
                    .previous_tool_use
                    .and_then(|entity| tool_uses.get(&entity).copied()),
                status: request.status.into(),
                sequence: request.sequence.0,
            },
        )?;
    }
    for (_, response) in world.query::<(Entity, &ModelResponse)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, response.turn)?;
        let (model_id, provider_id) = models
            .get(&response.model)
            .cloned()
            .ok_or_else(|| "ModelResponse references missing Model capability".to_string())?;
        insert(
            &mut events,
            session_id,
            Event::ModelResponse {
                request: world
                    .get::<ModelRequest>(response.request)
                    .map(|request| request.id.0)
                    .ok_or_else(|| "ModelResponse references missing ModelRequest".to_string())?,
                turn: turn_id.0,
                model_id,
                provider_id,
                generation: response.generation,
                response_id: response.response_id.clone(),
                api: response.api.into(),
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                stop_reason: response.stop_reason.into(),
                opaque_replay: response.opaque_replay.clone(),
                sequence: response.sequence.0,
            },
        )?;
    }
    for (_, tool_use) in world.query::<(Entity, &ToolUse)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, tool_use.turn)?;
        let (model_id, provider_id) = models
            .get(&tool_use.model)
            .cloned()
            .ok_or_else(|| "ToolUse references missing Model capability".to_string())?;
        let tool_id = tools
            .get(&tool_use.tool)
            .copied()
            .ok_or_else(|| "ToolUse references missing ToolDefinition capability".to_string())?;
        insert(
            &mut events,
            session_id,
            Event::ToolUse {
                id: tool_use.id.0,
                turn: turn_id.0,
                tool_id,
                model_id,
                provider_id,
                generation: tool_use.generation,
                input: tool_use.input.clone(),
                status: tool_use.status.into(),
                sequence: tool_use.sequence.0,
            },
        )?;
    }
    for (_, outcome) in world.query::<(Entity, &ToolOutcome)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, outcome.turn)?;
        let tool_use = tool_uses
            .get(&outcome.tool_use)
            .copied()
            .ok_or_else(|| "ToolOutcome references missing ToolUse".to_string())?;
        insert(
            &mut events,
            session_id,
            Event::ToolOutcome {
                tool_use,
                tool_call_id: outcome.tool_call_id.0,
                turn: turn_id.0,
                generation: outcome.generation,
                output: outcome.output.clone(),
                sequence: outcome.sequence.0,
            },
        )?;
    }
    for (_, outcome) in world.query::<(Entity, &TurnCompleted)>().iter(world) {
        insert_turn_outcome(
            &mut events,
            &turns,
            &sessions,
            outcome.turn,
            |sequence| {
                Ok(Event::TurnCompleted {
                    turn: turn_id(&turns, outcome.turn)?.0,
                    generation: outcome.generation,
                    sequence,
                })
            },
            outcome.sequence.0,
        )?;
    }
    for (_, outcome) in world.query::<(Entity, &TurnCancelled)>().iter(world) {
        insert_turn_outcome(
            &mut events,
            &turns,
            &sessions,
            outcome.turn,
            |sequence| {
                Ok(Event::TurnCancelled {
                    turn: turn_id(&turns, outcome.turn)?.0,
                    generation: outcome.generation,
                    sequence,
                })
            },
            outcome.sequence.0,
        )?;
    }
    for (_, outcome) in world.query::<(Entity, &TurnInterrupted)>().iter(world) {
        insert_turn_outcome(
            &mut events,
            &turns,
            &sessions,
            outcome.turn,
            |sequence| {
                Ok(Event::TurnInterrupted {
                    turn: turn_id(&turns, outcome.turn)?.0,
                    generation: outcome.generation,
                    sequence,
                })
            },
            outcome.sequence.0,
        )?;
    }
    for (_, outcome) in world.query::<(Entity, &TurnFailed)>().iter(world) {
        insert_turn_outcome(
            &mut events,
            &turns,
            &sessions,
            outcome.turn,
            |sequence| {
                Ok(Event::TurnFailed {
                    turn: turn_id(&turns, outcome.turn)?.0,
                    generation: outcome.generation,
                    failure: (&outcome.failure).into(),
                    sequence,
                })
            },
            outcome.sequence.0,
        )?;
    }
    for (_, branch) in world.query::<(Entity, &BranchSelection)>().iter(world) {
        let session_id = sessions
            .get(&branch.session)
            .map(|(id, _)| *id)
            .ok_or_else(|| "BranchSelection references missing Session".to_string())?;
        insert(
            &mut events,
            session_id,
            Event::BranchSelection {
                head: turn_id(&turns, branch.head)?.0,
                sequence: branch.sequence.0,
            },
        )?;
    }
    for (_, change) in world.query::<(Entity, &ModelChange)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, change.turn)?;
        let (model_id, provider_id) = models
            .get(&change.model)
            .cloned()
            .ok_or_else(|| "ModelChange references missing Model capability".to_string())?;
        insert(
            &mut events,
            session_id,
            Event::ModelChange {
                turn: turn_id.0,
                model_id,
                provider_id,
                sequence: change.sequence.0,
            },
        )?;
    }
    for (_, compaction) in world.query::<(Entity, &Compaction)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, compaction.turn)?;
        insert(
            &mut events,
            session_id,
            Event::Compaction {
                turn: turn_id.0,
                summary: compaction.summary.clone(),
                sequence: compaction.sequence.0,
            },
        )?;
    }
    for (_, recovery) in world.query::<(Entity, &Recovery)>().iter(world) {
        let (session_id, turn_id) = turn_context(&turns, &sessions, recovery.turn)?;
        insert(
            &mut events,
            session_id,
            Event::Recovery {
                turn: turn_id.0,
                text: recovery.text.clone(),
                sequence: recovery.sequence.0,
            },
        )?;
    }
    Ok(events)
}

fn insert(
    events: &mut HashMap<String, EventEntry>,
    session: SessionId,
    event: Event,
) -> Result<(), String> {
    let key = format!("{}:{}", session.0, event.key());
    let entry = EventEntry { session, event };
    if events.insert(key.clone(), entry).is_some()
        && !key.ends_with(":session")
        && !key.ends_with(":context-camera")
    {
        return Err(format!("duplicate persisted event key {key}"));
    }
    Ok(())
}

fn turn_context(
    turns: &HashMap<Entity, (Entity, TurnId)>,
    sessions: &HashMap<Entity, (SessionId, Option<Entity>)>,
    turn: Entity,
) -> Result<(SessionId, TurnId), String> {
    let (session, turn_id) = turns
        .get(&turn)
        .ok_or_else(|| format!("entity {turn:?} is missing Turn"))?;
    let session_id = sessions
        .get(session)
        .map(|(id, _)| *id)
        .ok_or_else(|| format!("Turn {turn:?} references missing Session"))?;
    Ok((session_id, *turn_id))
}

fn turn_id(turns: &HashMap<Entity, (Entity, TurnId)>, turn: Entity) -> Result<TurnId, String> {
    turns
        .get(&turn)
        .map(|(_, id)| *id)
        .ok_or_else(|| format!("entity {turn:?} is missing Turn"))
}

fn insert_turn_outcome<F>(
    events: &mut HashMap<String, EventEntry>,
    turns: &HashMap<Entity, (Entity, TurnId)>,
    sessions: &HashMap<Entity, (SessionId, Option<Entity>)>,
    turn: Entity,
    event: F,
    sequence: u64,
) -> Result<(), String>
where
    F: FnOnce(u64) -> Result<Event, String>,
{
    let (session, _) = turn_context(turns, sessions, turn)?;
    insert(events, session, event(sequence)?)
}
