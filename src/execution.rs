use std::collections::HashSet;

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, TaskPool, futures::check_ready},
};

use crate::harness::HarnessIds;
use crate::{
    Agent, AgentTool, AssistantMessage, ContextCamera, ContextDocument, ContextEntry, MajinSet,
    MajinStartupSet, Model, ModelApi, ModelOutput, ModelReply, ModelRequest, ModelRequestId,
    ModelResponse, ModelResult, ModelStopReason, ModelUsage, PersistentContextCamera,
    ProjectionError, Provider, ProviderFailure, ProviderId, Sequence, ToolDefinition, ToolFailure,
    ToolId, ToolOutcome, ToolResult, ToolUse, Turn, TurnCancelled, TurnCompleted, TurnFailed,
    TurnFailure, TurnInterrupted, WorkStatus, project_context_for,
};

pub(crate) fn configure(app: &mut App) {
    app.add_message::<ModelResult>()
        .add_message::<ToolResult>()
        .add_systems(
            Startup,
            register_fake_capabilities.in_set(MajinStartupSet::Harness),
        )
        .add_systems(
            Update,
            (dispatch_model_requests, dispatch_tool_uses).in_set(MajinSet::Dispatch),
        )
        .add_systems(
            Update,
            (
                collect_model_tasks,
                collect_tool_tasks,
                apply_model_results,
                apply_tool_results,
            )
                .chain()
                .in_set(MajinSet::Apply),
        );
}

#[derive(Component, Clone, Copy)]
struct ProviderExecutor(fn(ProviderInput) -> Task<Result<ModelOutput, ProviderFailure>>);

#[derive(Component, Clone, Copy)]
struct ToolExecutor(fn(ToolInput) -> Task<Result<String, ToolFailure>>);

struct ProviderInput {
    request_id: ModelRequestId,
    context: ContextDocument,
    continuation: bool,
    tools: Vec<ExposedTool>,
}

struct ExposedTool {
    id: ToolId,
    name: String,
    description: String,
}

struct ToolInput {
    input: String,
}

#[derive(Component)]
struct ModelTask(Task<ModelResult>);

#[derive(Component)]
struct ToolTask(Task<ToolResult>);

#[derive(SystemParam)]
struct ModelApply<'w, 's> {
    requests: Query<'w, 's, &'static mut ModelRequest>,
    turns: Query<'w, 's, &'static Turn>,
    agents: Query<'w, 's, &'static Agent>,
    agent_tools: Query<'w, 's, &'static AgentTool>,
    tools: Query<'w, 's, (Entity, &'static ToolDefinition)>,
}

#[derive(SystemParam)]
struct ToolApply<'w, 's> {
    tool_uses: Query<'w, 's, &'static mut ToolUse>,
    turns: Query<'w, 's, &'static Turn>,
}

type ModelTaskQueries<'w, 's> = ParamSet<
    'w,
    's,
    (
        Query<'w, 's, (Entity, &'static mut ModelTask)>,
        Query<'w, 's, Entity, Added<ModelTask>>,
    ),
>;

type ToolTaskQueries<'w, 's> = ParamSet<
    'w,
    's,
    (
        Query<'w, 's, (Entity, &'static mut ToolTask)>,
        Query<'w, 's, Entity, Added<ToolTask>>,
    ),
>;

#[derive(SystemParam)]
struct ModelTaskCollector<'w, 's> {
    tasks: ModelTaskQueries<'w, 's>,
}

#[derive(SystemParam)]
struct ToolTaskCollector<'w, 's> {
    tasks: ToolTaskQueries<'w, 's>,
}

pub(crate) fn cancel_model_task(world: &mut World, work: Entity) {
    world.entity_mut(work).remove::<ModelTask>();
}

pub(crate) fn cancel_tool_task(world: &mut World, work: Entity) {
    world.entity_mut(work).remove::<ToolTask>();
}

fn dispatch_model_requests(world: &mut World) {
    let requests: Vec<_> = {
        let mut query = world.query_filtered::<(Entity, &ModelRequest), Without<ModelTask>>();
        query
            .iter(world)
            .filter(|(_, request)| request.status == WorkStatus::Pending)
            .map(|(work, request)| (work, request.clone()))
            .collect()
    };
    for (work, request) in requests {
        let Some(turn) = world.get::<Turn>(request.turn).cloned() else {
            continue;
        };
        if turn.generation != request.generation || turn_finished(world, request.turn) {
            continue;
        }
        let Some(executor) = provider_executor(world, &request) else {
            fail_request(
                world,
                work,
                &request,
                ProviderFailure {
                    message: "Provider capability is unavailable.".into(),
                },
            );
            continue;
        };
        let Some(context_camera) = persistent_context_camera(world, request.agent, turn.session)
        else {
            fail_request(
                world,
                work,
                &request,
                ProviderFailure {
                    message: "Persistent Context camera is unavailable.".into(),
                },
            );
            continue;
        };
        let context = match project_context_for(world, context_camera) {
            Ok(context) => context,
            Err(error) => {
                fail_request(
                    world,
                    work,
                    &request,
                    ProviderFailure {
                        message: projection_failure(error).into(),
                    },
                );
                continue;
            }
        };
        let task = (executor.0)(ProviderInput {
            request_id: request.id,
            context,
            continuation: request.previous_tool_use.is_some(),
            tools: exposed_tools(world, request.agent),
        });
        let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            ModelResult {
                session: turn.session,
                turn: request.turn,
                work,
                request_id: request.id,
                generation: request.generation,
                result: task.await,
            }
        });
        world.entity_mut(work).insert(ModelTask(task));
        world
            .get_mut::<ModelRequest>(work)
            .expect("model work")
            .status = WorkStatus::Running;
    }
}

fn exposed_tools(world: &mut World, agent: Entity) -> Vec<ExposedTool> {
    let mut exposures = world.query::<&AgentTool>();
    let mut tools: Vec<_> = exposures
        .iter(world)
        .filter(|exposure| exposure.agent == agent)
        .filter_map(|exposure| {
            world
                .get::<ToolDefinition>(exposure.tool)
                .map(|tool| (exposure.order, exposure.tool.to_bits(), tool))
        })
        .map(|(order, entity, tool)| {
            (
                order,
                entity,
                ExposedTool {
                    id: tool.tool_id,
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                },
            )
        })
        .collect();
    tools.sort_by_key(|(order, entity, _)| (*order, *entity));
    tools.into_iter().map(|(_, _, tool)| tool).collect()
}

fn dispatch_tool_uses(world: &mut World) {
    let tool_uses: Vec<_> = {
        let mut query = world.query_filtered::<(Entity, &ToolUse), Without<ToolTask>>();
        query
            .iter(world)
            .filter(|(_, tool_use)| tool_use.status == WorkStatus::Pending)
            .map(|(work, tool_use)| (work, tool_use.clone()))
            .collect()
    };
    for (work, tool_use) in tool_uses {
        let Some(turn) = world.get::<Turn>(tool_use.turn).cloned() else {
            continue;
        };
        if turn.generation != tool_use.generation || turn_finished(world, tool_use.turn) {
            continue;
        }
        let Some(executor) = world
            .get::<ToolDefinition>(tool_use.tool)
            .and_then(|_| world.get::<ToolExecutor>(tool_use.tool))
            .copied()
        else {
            fail_tool_use(
                world,
                work,
                &tool_use,
                ToolFailure {
                    message: "Tool capability is unavailable.".into(),
                },
            );
            continue;
        };
        let task = (executor.0)(ToolInput {
            input: tool_use.input.clone(),
        });
        let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            ToolResult {
                session: turn.session,
                turn: tool_use.turn,
                work,
                tool_call_id: tool_use.id,
                generation: tool_use.generation,
                result: task.await,
            }
        });
        world.entity_mut(work).insert(ToolTask(task));
        world.get_mut::<ToolUse>(work).expect("tool work").status = WorkStatus::Running;
    }
}

fn collect_model_tasks(
    mut commands: Commands,
    mut collector: ModelTaskCollector,
    mut results: MessageWriter<ModelResult>,
) {
    let added: HashSet<_> = collector.tasks.p1().iter().collect();
    for (work, mut task) in &mut collector.tasks.p0() {
        if added.contains(&work) {
            continue;
        }
        if let Some(result) = check_ready(&mut task.0) {
            results.write(result);
            commands.entity(work).remove::<ModelTask>();
        }
    }
}

fn collect_tool_tasks(
    mut commands: Commands,
    mut collector: ToolTaskCollector,
    mut results: MessageWriter<ToolResult>,
) {
    let added: HashSet<_> = collector.tasks.p1().iter().collect();
    for (work, mut task) in &mut collector.tasks.p0() {
        if added.contains(&work) {
            continue;
        }
        if let Some(result) = check_ready(&mut task.0) {
            results.write(result);
            commands.entity(work).remove::<ToolTask>();
        }
    }
}

fn apply_model_results(
    mut commands: Commands,
    mut results: MessageReader<ModelResult>,
    mut state: ModelApply,
    mut ids: ResMut<HarnessIds>,
) {
    let mut accepted = HashSet::new();
    for result in results.read() {
        let Ok(mut request) = state.requests.get_mut(result.work) else {
            continue;
        };
        let Ok(turn) = state.turns.get(result.turn) else {
            continue;
        };
        if request.id != result.request_id
            || request.turn != result.turn
            || request.generation != result.generation
            || request.status != WorkStatus::Running
            || turn.session != result.session
            || turn.generation != result.generation
            || !accepted.insert(result.work)
        {
            continue;
        }
        match &result.result {
            Err(failure) => {
                fail_running_request(
                    &mut commands,
                    &mut request,
                    result,
                    failure.clone(),
                    ids.sequence(),
                );
            }
            Ok(output) => {
                let request_fields = (
                    request.turn,
                    request.agent,
                    request.model,
                    request.provider,
                    request.generation,
                );
                let response_sequence = ids.sequence();
                commands.spawn(ModelResponse {
                    request: result.work,
                    turn: request_fields.0,
                    model: request_fields.2,
                    generation: request_fields.4,
                    provider: request_fields.3,
                    response_id: output.response_id.clone(),
                    api: output.api,
                    usage: output.usage,
                    stop_reason: output.stop_reason,
                    opaque_replay: output.opaque_replay.clone(),
                    sequence: response_sequence,
                });
                let valid = matches!(
                    (&output.reply, output.stop_reason, request.previous_tool_use),
                    (ModelReply::ToolCall { .. }, ModelStopReason::ToolUse, None)
                        | (ModelReply::Final { .. }, ModelStopReason::Complete, Some(_))
                );
                if !valid {
                    fail_running_request(
                        &mut commands,
                        &mut request,
                        result,
                        ProviderFailure {
                            message: "Provider output does not match the request continuation."
                                .into(),
                        },
                        ids.sequence(),
                    );
                    continue;
                }
                request.status = WorkStatus::Succeeded;
                commands.entity(result.work).remove::<ModelTask>();
                match &output.reply {
                    ModelReply::ToolCall { tool_id, input } => {
                        if let Some(content) = &output.assistant_content {
                            commands.spawn(AssistantMessage {
                                id: ids.message(),
                                turn: request_fields.0,
                                sequence: ids.sequence(),
                                text: content.clone(),
                            });
                        }
                        let Some(tool) = state
                            .tools
                            .iter()
                            .filter(|(entity, tool)| {
                                tool.tool_id == *tool_id
                                    && state.agent_tools.iter().any(|exposure| {
                                        exposure.agent == request_fields.1
                                            && exposure.tool == *entity
                                    })
                            })
                            .map(|(entity, _)| entity)
                            .min_by_key(|entity| entity.to_bits())
                        else {
                            fail_running_request(
                                &mut commands,
                                &mut request,
                                result,
                                ProviderFailure {
                                    message: "Requested Tool is not exposed by the Agent.".into(),
                                },
                                ids.sequence(),
                            );
                            continue;
                        };
                        if !matches!(
                            state.agents.get(request_fields.1),
                            Ok(agent) if agent.model == request_fields.2
                        ) {
                            fail_running_request(
                                &mut commands,
                                &mut request,
                                result,
                                ProviderFailure {
                                    message: "Model request Agent is unavailable.".into(),
                                },
                                ids.sequence(),
                            );
                            continue;
                        }
                        commands.spawn(ToolUse {
                            id: ids.tool_call(),
                            turn: request_fields.0,
                            agent: request_fields.1,
                            tool,
                            model: request_fields.2,
                            provider: request_fields.3,
                            generation: request_fields.4,
                            input: input.clone(),
                            status: WorkStatus::Pending,
                            sequence: ids.sequence(),
                        });
                    }
                    ModelReply::Final { text } => {
                        commands.spawn(AssistantMessage {
                            id: ids.message(),
                            turn: request_fields.0,
                            sequence: ids.sequence(),
                            text: text.clone(),
                        });
                        commands.spawn(TurnCompleted {
                            turn: request_fields.0,
                            generation: request_fields.4,
                            sequence: ids.sequence(),
                        });
                    }
                }
            }
        }
    }
}

fn fail_running_request(
    commands: &mut Commands,
    request: &mut ModelRequest,
    result: &ModelResult,
    failure: ProviderFailure,
    sequence: Sequence,
) {
    request.status = WorkStatus::Failed;
    commands.entity(result.work).remove::<ModelTask>();
    commands.spawn(TurnFailed {
        turn: result.turn,
        generation: result.generation,
        failure: TurnFailure::Provider(failure),
        sequence,
    });
}

fn apply_tool_results(
    mut commands: Commands,
    mut results: MessageReader<ToolResult>,
    mut state: ToolApply,
    mut ids: ResMut<HarnessIds>,
) {
    let mut accepted = HashSet::new();
    for result in results.read() {
        let Ok(mut tool_use) = state.tool_uses.get_mut(result.work) else {
            continue;
        };
        let Ok(turn) = state.turns.get(result.turn) else {
            continue;
        };
        if tool_use.id != result.tool_call_id
            || tool_use.turn != result.turn
            || tool_use.generation != result.generation
            || tool_use.status != WorkStatus::Running
            || turn.session != result.session
            || turn.generation != result.generation
            || !accepted.insert(result.work)
        {
            continue;
        }
        match &result.result {
            Err(failure) => {
                tool_use.status = WorkStatus::Failed;
                commands.entity(result.work).remove::<ToolTask>();
                commands.spawn(TurnFailed {
                    turn: result.turn,
                    generation: result.generation,
                    failure: TurnFailure::Tool(failure.clone()),
                    sequence: ids.sequence(),
                });
            }
            Ok(output) => {
                let request_id = ids.model_request();
                let fields = (
                    tool_use.id,
                    tool_use.turn,
                    tool_use.agent,
                    tool_use.model,
                    tool_use.provider,
                    tool_use.generation,
                );
                tool_use.status = WorkStatus::Succeeded;
                commands.entity(result.work).remove::<ToolTask>();
                commands.spawn(ToolOutcome {
                    tool_use: result.work,
                    tool_call_id: fields.0,
                    turn: fields.1,
                    generation: fields.5,
                    output: output.clone(),
                    sequence: ids.sequence(),
                });
                commands.spawn(ModelRequest {
                    id: request_id,
                    turn: fields.1,
                    agent: fields.2,
                    model: fields.3,
                    provider: fields.4,
                    generation: fields.5,
                    previous_tool_use: Some(result.work),
                    status: WorkStatus::Pending,
                    sequence: ids.sequence(),
                });
            }
        }
    }
}

fn provider_executor(world: &World, request: &ModelRequest) -> Option<ProviderExecutor> {
    let agent = world.get::<Agent>(request.agent)?;
    let model = world.get::<Model>(request.model)?;
    if agent.model != request.model || model.provider != request.provider {
        return None;
    }
    world.get::<Provider>(request.provider)?;
    world.get::<ProviderExecutor>(request.provider).copied()
}

fn persistent_context_camera(
    world: &mut World,
    agent: Entity,
    session: Entity,
) -> Option<ContextCamera> {
    world
        .query::<(Entity, &ContextCamera, &PersistentContextCamera)>()
        .iter(world)
        .filter(|(_, camera, _)| camera.agent == agent && camera.session == session)
        .min_by_key(|(entity, _, _)| entity.to_bits())
        .map(|(_, camera, _)| *camera)
}

pub(crate) fn sync_persistent_context_camera(
    world: &mut World,
    agent: Entity,
    session: Entity,
    head: Option<Entity>,
) {
    let cameras: Vec<_> = world
        .query::<(Entity, &ContextCamera, &PersistentContextCamera)>()
        .iter(world)
        .filter(|(_, camera, _)| camera.agent == agent && camera.session == session)
        .map(|(entity, _, _)| entity)
        .collect();
    for camera in cameras {
        world
            .get_mut::<ContextCamera>(camera)
            .expect("context camera")
            .head = head;
    }
}

fn turn_finished(world: &mut World, turn: Entity) -> bool {
    let mut completed = world.query::<&TurnCompleted>();
    let mut cancelled = world.query::<&TurnCancelled>();
    let mut failed = world.query::<&TurnFailed>();
    let mut interrupted = world.query::<&TurnInterrupted>();
    completed.iter(world).any(|outcome| outcome.turn == turn)
        || cancelled.iter(world).any(|outcome| outcome.turn == turn)
        || failed.iter(world).any(|outcome| outcome.turn == turn)
        || interrupted.iter(world).any(|outcome| outcome.turn == turn)
}

fn fail_request(world: &mut World, work: Entity, request: &ModelRequest, failure: ProviderFailure) {
    let sequence = world.resource_mut::<HarnessIds>().sequence();
    world
        .get_mut::<ModelRequest>(work)
        .expect("model work")
        .status = WorkStatus::Failed;
    cancel_model_task(world, work);
    world.spawn(TurnFailed {
        turn: request.turn,
        generation: request.generation,
        failure: TurnFailure::Provider(failure),
        sequence,
    });
}

fn fail_tool_use(world: &mut World, work: Entity, tool_use: &ToolUse, failure: ToolFailure) {
    let sequence = world.resource_mut::<HarnessIds>().sequence();
    world.get_mut::<ToolUse>(work).expect("tool work").status = WorkStatus::Failed;
    cancel_tool_task(world, work);
    world.spawn(TurnFailed {
        turn: tool_use.turn,
        generation: tool_use.generation,
        failure: TurnFailure::Tool(failure),
        sequence,
    });
}

fn projection_failure(error: ProjectionError) -> &'static str {
    match error {
        ProjectionError::MissingCamera => "Context camera does not exist.",
        ProjectionError::MissingTurn => "Context branch contains a missing Turn.",
        ProjectionError::CrossSession => "Context branch crosses into another Session.",
        ProjectionError::Cycle => "Context branch contains a cycle.",
        ProjectionError::MissingModel => "Context branch references a missing Model.",
    }
}

struct FakeNativeRequest {
    input: ProviderInput,
}

impl FakeNativeRequest {
    fn from_input(input: ProviderInput) -> Self {
        Self { input }
    }

    fn output(self) -> Result<ModelOutput, ProviderFailure> {
        let reply = if self.input.continuation {
            ModelReply::Final {
                text: "Fake harness completed the request.".into(),
            }
        } else {
            let tool = self.input.tools.first().ok_or_else(|| ProviderFailure {
                message: "Agent exposes no Tool for the fake Provider.".into(),
            })?;
            let input = self
                .input
                .context
                .entries
                .into_iter()
                .rev()
                .find_map(|entry| match entry {
                    ContextEntry::User(text) => Some(text),
                    _ => None,
                })
                .unwrap_or_default();
            let _ = (&tool.name, &tool.description);
            ModelReply::ToolCall {
                tool_id: tool.id,
                input,
            }
        };
        let (assistant_content, stop_reason, opaque_replay) = match &reply {
            ModelReply::ToolCall { input, .. } => (
                Some("Calling fake_tool.".into()),
                ModelStopReason::ToolUse,
                format!("fake-tool-call:{input}"),
            ),
            ModelReply::Final { text } => (
                Some(text.clone()),
                ModelStopReason::Complete,
                "fake-final".into(),
            ),
        };
        Ok(ModelOutput {
            reply,
            assistant_content,
            response_id: format!("fake-response-{}", self.input.request_id.0),
            api: ModelApi::Fake,
            usage: ModelUsage {
                input_tokens: 0,
                output_tokens: 1,
            },
            stop_reason,
            opaque_replay,
        })
    }
}

fn fake_provider(input: ProviderInput) -> Task<Result<ModelOutput, ProviderFailure>> {
    AsyncComputeTaskPool::get_or_init(TaskPool::new)
        .spawn(async move { FakeNativeRequest::from_input(input).output() })
}

fn fake_tool(input: ToolInput) -> Task<Result<String, ToolFailure>> {
    AsyncComputeTaskPool::get_or_init(TaskPool::new)
        .spawn(async move { Ok(format!("Fake tool completed: {}", input.input)) })
}

fn register_fake_capabilities(world: &mut World) {
    reconcile_fake_capabilities(world);
}

pub(crate) fn reconcile_fake_capabilities(world: &mut World) -> Entity {
    let provider = find_provider(world).unwrap_or_else(|| {
        world
            .spawn(Provider {
                provider_id: ProviderId(1),
            })
            .id()
    });
    world.entity_mut(provider).insert((
        Provider {
            provider_id: ProviderId(1),
        },
        ProviderExecutor(fake_provider),
    ));

    let model = find_model(world, provider).unwrap_or_else(|| {
        world
            .spawn(Model {
                provider,
                model_id: "fake-model".into(),
            })
            .id()
    });
    world.entity_mut(model).insert(Model {
        provider,
        model_id: "fake-model".into(),
    });

    let tool = find_tool(world).unwrap_or_else(|| {
        world
            .spawn(ToolDefinition {
                tool_id: ToolId(1),
                name: "fake_tool".into(),
                description: "Temporary fake tool capability.".into(),
            })
            .id()
    });
    world.entity_mut(tool).insert((
        ToolDefinition {
            tool_id: ToolId(1),
            name: "fake_tool".into(),
            description: "Temporary fake tool capability.".into(),
        },
        ToolExecutor(fake_tool),
    ));

    let agent = find_agent(world, model).unwrap_or_else(|| world.spawn(Agent { model }).id());
    world.entity_mut(agent).insert(Agent { model });

    let exposure = world
        .query::<(Entity, &AgentTool)>()
        .iter(world)
        .find(|(_, exposure)| exposure.agent == agent && exposure.tool == tool)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| {
            world
                .spawn(AgentTool {
                    agent,
                    tool,
                    order: 0,
                })
                .id()
        });
    world.entity_mut(exposure).insert(AgentTool {
        agent,
        tool,
        order: 0,
    });
    agent
}

fn find_provider(world: &mut World) -> Option<Entity> {
    world
        .query::<(Entity, &Provider)>()
        .iter(world)
        .filter(|(_, provider)| provider.provider_id == ProviderId(1))
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.to_bits())
}

fn find_model(world: &mut World, provider: Entity) -> Option<Entity> {
    world
        .query::<(Entity, &Model)>()
        .iter(world)
        .filter(|(_, model)| model.provider == provider)
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.to_bits())
}

fn find_tool(world: &mut World) -> Option<Entity> {
    world
        .query::<(Entity, &ToolDefinition)>()
        .iter(world)
        .filter(|(_, tool)| tool.tool_id == ToolId(1))
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.to_bits())
}

fn find_agent(world: &mut World, model: Entity) -> Option<Entity> {
    world
        .query::<(Entity, &Agent)>()
        .iter(world)
        .filter(|(_, agent)| agent.model == model)
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.to_bits())
}
