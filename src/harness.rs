use bevy::{
    ecs::{entity::MapEntities, reflect::ReflectMapEntities, system::Command},
    prelude::*,
};

use crate::execution;

pub struct HarnessPlugin;

impl Plugin for HarnessPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CommandResult>();
        execution::configure(app);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct ProviderId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct ToolId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct TurnId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct ModelRequestId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct ToolCallId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
pub struct MessageId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
pub struct Sequence(pub u64);

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct Agent {
    #[entities]
    pub model: Entity,
}

#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct Provider {
    pub provider_id: ProviderId,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct Model {
    #[entities]
    pub provider: Entity,
    pub model_id: String,
}

#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct ToolDefinition {
    pub tool_id: ToolId,
    pub name: String,
    pub description: String,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct AgentTool {
    #[entities]
    pub agent: Entity,
    #[entities]
    pub tool: Entity,
    pub order: u32,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct Session {
    pub id: SessionId,
    #[entities]
    pub active_head: Option<Entity>,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct Turn {
    pub id: TurnId,
    #[entities]
    pub session: Entity,
    #[entities]
    pub parent: Option<Entity>,
    pub sequence: Sequence,
    pub generation: u64,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct UserMessage {
    pub id: MessageId,
    #[entities]
    pub turn: Entity,
    pub sequence: Sequence,
    pub text: String,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct AssistantMessage {
    pub id: MessageId,
    #[entities]
    pub turn: Entity,
    pub sequence: Sequence,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum WorkStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct ModelRequest {
    pub id: ModelRequestId,
    #[entities]
    pub turn: Entity,
    #[entities]
    pub agent: Entity,
    #[entities]
    pub model: Entity,
    #[entities]
    pub provider: Entity,
    pub generation: u64,
    #[entities]
    pub previous_tool_use: Option<Entity>,
    pub status: WorkStatus,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct ModelResponse {
    #[entities]
    pub request: Entity,
    #[entities]
    pub turn: Entity,
    #[entities]
    pub model: Entity,
    pub generation: u64,
    #[entities]
    pub provider: Entity,
    pub response_id: String,
    pub api: ModelApi,
    pub usage: ModelUsage,
    pub stop_reason: ModelStopReason,
    pub opaque_replay: String,
    pub sequence: Sequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum ModelStopReason {
    ToolUse,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum ModelApi {
    Fake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub struct ModelUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOutput {
    pub reply: ModelReply,
    pub assistant_content: Option<String>,
    pub response_id: String,
    pub api: ModelApi,
    pub usage: ModelUsage,
    pub stop_reason: ModelStopReason,
    pub opaque_replay: String,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct ToolUse {
    pub id: ToolCallId,
    #[entities]
    pub turn: Entity,
    #[entities]
    pub agent: Entity,
    #[entities]
    pub tool: Entity,
    #[entities]
    pub model: Entity,
    #[entities]
    pub provider: Entity,
    pub generation: u64,
    pub input: String,
    pub status: WorkStatus,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct ToolOutcome {
    #[entities]
    pub tool_use: Entity,
    pub tool_call_id: ToolCallId,
    #[entities]
    pub turn: Entity,
    pub generation: u64,
    pub output: String,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct TurnCompleted {
    #[entities]
    pub turn: Entity,
    pub generation: u64,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct TurnCancelled {
    #[entities]
    pub turn: Entity,
    pub generation: u64,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct TurnInterrupted {
    #[entities]
    pub turn: Entity,
    pub generation: u64,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct TurnFailed {
    #[entities]
    pub turn: Entity,
    pub generation: u64,
    pub failure: TurnFailure,
    pub sequence: Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub enum TurnFailure {
    Provider(ProviderFailure),
    Tool(ToolFailure),
    Recovery(RecoveryFailure),
}

#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub struct ProviderFailure {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub struct ToolFailure {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub struct RecoveryFailure {
    pub message: String,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct BranchSelection {
    #[entities]
    pub session: Entity,
    #[entities]
    pub head: Entity,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct ModelChange {
    #[entities]
    pub turn: Entity,
    #[entities]
    pub model: Entity,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct Compaction {
    #[entities]
    pub turn: Entity,
    pub summary: String,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect, MapEntities)]
#[reflect(Component, MapEntities)]
pub struct Recovery {
    #[entities]
    pub turn: Entity,
    pub text: String,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct PersistenceFailure {
    pub message: String,
    pub sequence: Sequence,
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct ActiveSession(pub Entity);

#[derive(Resource, Debug, Clone, Copy)]
pub struct ActiveAgent(pub Entity);

#[derive(Resource, Debug, Clone, Copy)]
pub struct HarnessReady;

#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    PromptSubmitted {
        session: Entity,
        turn: Entity,
    },
    PromptRejected {
        session: Entity,
        failure: CommandFailure,
    },
    SessionSelected {
        session: Entity,
    },
    SessionRejected {
        session: Entity,
        failure: CommandFailure,
    },
    BranchSelected {
        session: Entity,
        head: Entity,
    },
    BranchRejected {
        session: Entity,
        head: Entity,
        failure: CommandFailure,
    },
    TurnInterrupted {
        turn: Entity,
        generation: u64,
    },
    InterruptRejected {
        turn: Entity,
        failure: CommandFailure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFailure {
    EmptyPrompt,
    MissingSession,
    MissingAgent,
    MissingTurn,
    TurnOutsideSession,
    TurnNotActive,
    ActiveTurn,
    TurnFinished,
}

#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct ModelResult {
    pub session: Entity,
    pub turn: Entity,
    pub work: Entity,
    pub request_id: ModelRequestId,
    pub generation: u64,
    pub result: Result<ModelOutput, ProviderFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelReply {
    ToolCall { tool_id: ToolId, input: String },
    Final { text: String },
}

#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub session: Entity,
    pub turn: Entity,
    pub work: Entity,
    pub tool_call_id: ToolCallId,
    pub generation: u64,
    pub result: Result<String, ToolFailure>,
}

#[derive(Resource, Reflect)]
#[reflect(Resource)]
pub(crate) struct HarnessIds {
    next_turn: u64,
    next_message: u64,
    next_model_request: u64,
    next_tool_call: u64,
    next_sequence: u64,
}

impl Default for HarnessIds {
    fn default() -> Self {
        Self {
            next_turn: 1,
            next_message: 1,
            next_model_request: 1,
            next_tool_call: 1,
            next_sequence: 1,
        }
    }
}

impl HarnessIds {
    pub(crate) fn turn(&mut self) -> TurnId {
        let id = TurnId(self.next_turn);
        self.next_turn += 1;
        id
    }

    pub(crate) fn message(&mut self) -> MessageId {
        let id = MessageId(self.next_message);
        self.next_message += 1;
        id
    }

    pub(crate) fn model_request(&mut self) -> ModelRequestId {
        let id = ModelRequestId(self.next_model_request);
        self.next_model_request += 1;
        id
    }

    pub(crate) fn tool_call(&mut self) -> ToolCallId {
        let id = ToolCallId(self.next_tool_call);
        self.next_tool_call += 1;
        id
    }

    pub(crate) fn sequence(&mut self) -> Sequence {
        let sequence = Sequence(self.next_sequence);
        self.next_sequence += 1;
        sequence
    }
}

pub struct SubmitPrompt {
    pub session: Entity,
    pub text: String,
}

impl Command for SubmitPrompt {
    type Out = ();

    fn apply(self, world: &mut World) {
        let text = self.text.trim();
        let Some(session) = world.get::<Session>(self.session).cloned() else {
            world.write_message(CommandResult::PromptRejected {
                session: self.session,
                failure: CommandFailure::MissingSession,
            });
            return;
        };
        if text.is_empty() {
            world.write_message(CommandResult::PromptRejected {
                session: self.session,
                failure: CommandFailure::EmptyPrompt,
            });
            return;
        }
        if session_has_active_work(world, self.session)
            || session
                .active_head
                .is_some_and(|head| !turn_finished(world, head))
        {
            world.write_message(CommandResult::PromptRejected {
                session: self.session,
                failure: CommandFailure::ActiveTurn,
            });
            return;
        }
        let Some(active_agent) = world.get_resource::<ActiveAgent>().copied() else {
            world.write_message(CommandResult::PromptRejected {
                session: self.session,
                failure: CommandFailure::MissingAgent,
            });
            return;
        };
        let Some((model, provider)) = (|| {
            let agent = world.get::<Agent>(active_agent.0)?;
            let model = world.get::<Model>(agent.model)?;
            Some((agent.model, model.provider))
        })() else {
            world.write_message(CommandResult::PromptRejected {
                session: self.session,
                failure: CommandFailure::MissingAgent,
            });
            return;
        };
        let parent = session.active_head;
        let (turn_id, turn_sequence, user_id, user_sequence, request_id, request_sequence) = {
            let mut ids = world.resource_mut::<HarnessIds>();
            (
                ids.turn(),
                ids.sequence(),
                ids.message(),
                ids.sequence(),
                ids.model_request(),
                ids.sequence(),
            )
        };
        let turn = world
            .spawn(Turn {
                id: turn_id,
                session: self.session,
                parent,
                sequence: turn_sequence,
                generation: 0,
            })
            .id();
        world.spawn(UserMessage {
            id: user_id,
            turn,
            sequence: user_sequence,
            text: text.to_owned(),
        });
        world.spawn(ModelRequest {
            id: request_id,
            turn,
            agent: active_agent.0,
            model,
            provider,
            generation: 0,
            previous_tool_use: None,
            status: WorkStatus::Pending,
            sequence: request_sequence,
        });
        world
            .get_mut::<Session>(self.session)
            .expect("validated session")
            .active_head = Some(turn);
        world.write_message(CommandResult::PromptSubmitted {
            session: self.session,
            turn,
        });
    }
}

pub struct InterruptTurn {
    pub turn: Entity,
}

impl Command for InterruptTurn {
    type Out = ();

    fn apply(self, world: &mut World) {
        let Some(turn) = world.get::<Turn>(self.turn).cloned() else {
            world.write_message(CommandResult::InterruptRejected {
                turn: self.turn,
                failure: CommandFailure::MissingTurn,
            });
            return;
        };
        let Some(session) = world.get::<Session>(turn.session) else {
            world.write_message(CommandResult::InterruptRejected {
                turn: self.turn,
                failure: CommandFailure::MissingSession,
            });
            return;
        };
        if session.active_head != Some(self.turn) {
            world.write_message(CommandResult::InterruptRejected {
                turn: self.turn,
                failure: CommandFailure::TurnNotActive,
            });
            return;
        }
        if turn_finished(world, self.turn) {
            world.write_message(CommandResult::InterruptRejected {
                turn: self.turn,
                failure: CommandFailure::TurnFinished,
            });
            return;
        }

        let model_work: Vec<_> = {
            let mut requests = world.query::<(Entity, &ModelRequest)>();
            requests
                .iter(world)
                .filter(|(_, request)| {
                    request.turn == self.turn
                        && matches!(request.status, WorkStatus::Pending | WorkStatus::Running)
                })
                .map(|(entity, _)| entity)
                .collect()
        };
        let tool_work: Vec<_> = {
            let mut tool_uses = world.query::<(Entity, &ToolUse)>();
            tool_uses
                .iter(world)
                .filter(|(_, tool_use)| {
                    tool_use.turn == self.turn
                        && matches!(tool_use.status, WorkStatus::Pending | WorkStatus::Running)
                })
                .map(|(entity, _)| entity)
                .collect()
        };
        for work in model_work {
            world
                .get_mut::<ModelRequest>(work)
                .expect("model work")
                .status = WorkStatus::Cancelled;
            execution::cancel_model_task(world, work);
        }
        for work in tool_work {
            world.get_mut::<ToolUse>(work).expect("tool work").status = WorkStatus::Cancelled;
            execution::cancel_tool_task(world, work);
        }
        let generation = {
            let mut active_turn = world.get_mut::<Turn>(self.turn).expect("validated turn");
            active_turn.generation += 1;
            active_turn.generation
        };
        let sequence = world.resource_mut::<HarnessIds>().sequence();
        world.spawn(TurnCancelled {
            turn: self.turn,
            generation,
            sequence,
        });
        world.write_message(CommandResult::TurnInterrupted {
            turn: self.turn,
            generation,
        });
    }
}

pub struct SelectSession {
    pub session: Entity,
}

impl Command for SelectSession {
    type Out = ();

    fn apply(self, world: &mut World) {
        if world.get::<Session>(self.session).is_some() {
            world.insert_resource(ActiveSession(self.session));
            world.write_message(CommandResult::SessionSelected {
                session: self.session,
            });
        } else {
            world.write_message(CommandResult::SessionRejected {
                session: self.session,
                failure: CommandFailure::MissingSession,
            });
        }
    }
}

pub struct SelectBranch {
    pub session: Entity,
    pub head: Entity,
}

impl Command for SelectBranch {
    type Out = ();

    fn apply(self, world: &mut World) {
        let Some(turn) = world.get::<Turn>(self.head) else {
            world.write_message(CommandResult::BranchRejected {
                session: self.session,
                head: self.head,
                failure: CommandFailure::MissingTurn,
            });
            return;
        };
        if turn.session != self.session {
            world.write_message(CommandResult::BranchRejected {
                session: self.session,
                head: self.head,
                failure: CommandFailure::TurnOutsideSession,
            });
            return;
        }
        if world.get::<Session>(self.session).is_none() {
            world.write_message(CommandResult::BranchRejected {
                session: self.session,
                head: self.head,
                failure: CommandFailure::MissingSession,
            });
            return;
        }
        let sequence = world.resource_mut::<HarnessIds>().sequence();
        world
            .get_mut::<Session>(self.session)
            .expect("validated session")
            .active_head = Some(self.head);
        world.spawn(BranchSelection {
            session: self.session,
            head: self.head,
            sequence,
        });
        world.write_message(CommandResult::BranchSelected {
            session: self.session,
            head: self.head,
        });
    }
}

fn session_has_active_work(world: &mut World, session: Entity) -> bool {
    let mut requests = world.query::<&ModelRequest>();
    if requests.iter(world).any(|request| {
        matches!(request.status, WorkStatus::Pending | WorkStatus::Running)
            && world
                .get::<Turn>(request.turn)
                .is_some_and(|turn| turn.session == session)
    }) {
        return true;
    }
    let mut tool_uses = world.query::<&ToolUse>();
    tool_uses.iter(world).any(|tool_use| {
        matches!(tool_use.status, WorkStatus::Pending | WorkStatus::Running)
            && world
                .get::<Turn>(tool_use.turn)
                .is_some_and(|turn| turn.session == session)
    })
}

fn turn_finished(world: &mut World, turn: Entity) -> bool {
    let mut completed = world.query::<&TurnCompleted>();
    let mut cancelled = world.query::<&TurnCancelled>();
    let mut failed = world.query::<&TurnFailed>();
    completed.iter(world).any(|outcome| outcome.turn == turn)
        || cancelled.iter(world).any(|outcome| outcome.turn == turn)
        || failed.iter(world).any(|outcome| outcome.turn == turn)
}
