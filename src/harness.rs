use bevy::{ecs::system::Command, prelude::*};

use crate::MajinStartupSet;

const FAKE_RESPONSE: &str = "Fake harness received the message. No agent is connected yet.";

pub struct HarnessPlugin;

impl Plugin for HarnessPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_harness.in_set(MajinStartupSet::Harness));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToolId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TurnId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelRequestId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToolCallId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sequence(pub u64);

#[derive(Component, Debug, Clone)]
pub struct Agent {
    pub model: Entity,
}

#[derive(Component, Debug, Clone)]
pub struct Provider {
    pub provider_id: ProviderId,
}

#[derive(Component, Debug, Clone)]
pub struct Model {
    pub provider: Entity,
    pub model_id: String,
}

#[derive(Component, Debug, Clone)]
pub struct ToolDefinition {
    pub tool_id: ToolId,
    pub name: String,
    pub description: String,
}

#[derive(Component, Debug, Clone)]
pub struct AgentTool {
    pub agent: Entity,
    pub tool: Entity,
    pub order: u32,
}

#[derive(Component, Debug, Clone)]
pub struct Session {
    pub id: SessionId,
    pub active_head: Option<Entity>,
}

#[derive(Component, Debug, Clone)]
pub struct Turn {
    pub id: TurnId,
    pub session: Entity,
    pub parent: Option<Entity>,
    pub sequence: Sequence,
    pub generation: u64,
}

#[derive(Component, Debug, Clone)]
pub struct UserMessage {
    pub id: MessageId,
    pub turn: Entity,
    pub sequence: Sequence,
    pub text: String,
}

#[derive(Component, Debug, Clone)]
pub struct AssistantMessage {
    pub id: MessageId,
    pub turn: Entity,
    pub sequence: Sequence,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Component, Debug, Clone)]
pub struct ModelRequest {
    pub id: ModelRequestId,
    pub turn: Entity,
    pub agent: Entity,
    pub model: Entity,
    pub provider: Entity,
    pub generation: u64,
    pub previous_tool_use: Option<Entity>,
    pub status: WorkStatus,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone)]
pub struct ModelResponse {
    pub request: Entity,
    pub turn: Entity,
    pub model: Entity,
    pub generation: u64,
    pub provider: Entity,
    pub response_id: String,
    pub api: ModelApi,
    pub usage: ModelUsage,
    pub stop_reason: ModelStopReason,
    pub opaque_replay: String,
    pub sequence: Sequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStopReason {
    ToolUse,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelApi {
    Fake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Component, Debug, Clone)]
pub struct ToolUse {
    pub id: ToolCallId,
    pub turn: Entity,
    pub agent: Entity,
    pub tool: Entity,
    pub model: Entity,
    pub provider: Entity,
    pub generation: u64,
    pub input: String,
    pub status: WorkStatus,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone)]
pub struct ToolOutcome {
    pub tool_use: Entity,
    pub tool_call_id: ToolCallId,
    pub turn: Entity,
    pub generation: u64,
    pub output: String,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone)]
pub struct TurnCompleted {
    pub turn: Entity,
    pub generation: u64,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone)]
pub struct TurnCancelled {
    pub turn: Entity,
    pub generation: u64,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone)]
pub struct TurnFailed {
    pub turn: Entity,
    pub generation: u64,
    pub failure: TurnFailure,
    pub sequence: Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnFailure {
    Provider(ProviderFailure),
    Tool(ToolFailure),
    Recovery(RecoveryFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFailure {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFailure {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryFailure {
    pub message: String,
}

#[derive(Component, Debug, Clone)]
pub struct BranchSelection {
    pub session: Entity,
    pub head: Entity,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone)]
pub struct ModelChange {
    pub turn: Entity,
    pub model: Entity,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone)]
pub struct Compaction {
    pub turn: Entity,
    pub summary: String,
    pub sequence: Sequence,
}

#[derive(Component, Debug, Clone)]
pub struct Recovery {
    pub turn: Entity,
    pub text: String,
    pub sequence: Sequence,
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct ActiveSession(pub Entity);

#[derive(Resource, Debug, Clone, Copy)]
pub struct ActiveAgent(pub Entity);

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

#[derive(Resource)]
pub(crate) struct HarnessIds {
    next_turn: u64,
    next_message: u64,
    next_sequence: u64,
}

impl Default for HarnessIds {
    fn default() -> Self {
        Self {
            next_turn: 1,
            next_message: 1,
            next_sequence: 1,
        }
    }
}

impl HarnessIds {
    fn turn(&mut self) -> TurnId {
        let id = TurnId(self.next_turn);
        self.next_turn += 1;
        id
    }

    fn message(&mut self) -> MessageId {
        let id = MessageId(self.next_message);
        self.next_message += 1;
        id
    }

    fn sequence(&mut self) -> Sequence {
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
        let Some(parent) = world
            .get::<Session>(self.session)
            .map(|session| session.active_head)
        else {
            return;
        };
        if text.is_empty() {
            return;
        }

        let (turn_id, turn_sequence, user_id, user_sequence, assistant_id, assistant_sequence) = {
            let mut ids = world.resource_mut::<HarnessIds>();
            (
                ids.turn(),
                ids.sequence(),
                ids.message(),
                ids.sequence(),
                ids.message(),
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
        world.spawn(AssistantMessage {
            id: assistant_id,
            turn,
            sequence: assistant_sequence,
            text: FAKE_RESPONSE.into(),
        });
        world
            .get_mut::<Session>(self.session)
            .expect("validated session")
            .active_head = Some(turn);
    }
}

pub struct InterruptTurn {
    pub turn: Entity,
}

pub struct SelectSession {
    pub session: Entity,
}

impl Command for SelectSession {
    type Out = ();

    fn apply(self, world: &mut World) {
        if world.get::<Session>(self.session).is_some() {
            world.insert_resource(ActiveSession(self.session));
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
            return;
        };
        if turn.session != self.session || world.get::<Session>(self.session).is_none() {
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
    }
}

fn register_harness(world: &mut World) {
    let provider = world
        .spawn(Provider {
            provider_id: ProviderId(1),
        })
        .id();
    let model = world
        .spawn(Model {
            provider,
            model_id: "fake-model".into(),
        })
        .id();
    let tool = world
        .spawn(ToolDefinition {
            tool_id: ToolId(1),
            name: "fake_tool".into(),
            description: "Temporary fake tool capability.".into(),
        })
        .id();
    let agent = world.spawn(Agent { model }).id();
    world.spawn(AgentTool {
        agent,
        tool,
        order: 0,
    });
    let session = world
        .spawn(Session {
            id: SessionId(1),
            active_head: None,
        })
        .id();

    world.insert_resource(HarnessIds::default());
    world.insert_resource(ActiveSession(session));
}
