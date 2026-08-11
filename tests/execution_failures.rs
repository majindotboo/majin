mod common;

use bevy::{
    ecs::system::Command,
    prelude::{App, Component, Entity, With},
};
use majin::{
    ActiveSession, AssistantMessage, InterruptTurn, ModelApi, ModelOutput, ModelRequest,
    ModelResponse, ModelResult, ModelStopReason, ModelUsage, Provider, ProviderFailure,
    ToolDefinition, ToolOutcome, ToolResult, ToolUse, Turn, TurnCancelled, TurnCompleted,
    TurnFailed, TurnFailure, WorkStatus,
};
use pretty_assertions::assert_eq;
use rstest::rstest;

use common::{app, update_until};

fn active_session(app: &App) -> Entity {
    app.world().resource::<ActiveSession>().0
}

fn single_entity<T: Component>(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<T>>()
        .single(app.world())
        .expect("one matching entity")
}

fn tool_call_output(input: &str) -> ModelOutput {
    ModelOutput {
        reply: majin::ModelReply::ToolCall {
            tool_id: majin::ToolId(1),
            input: input.into(),
        },
        assistant_content: Some("injected tool call".into()),
        response_id: "injected-response".into(),
        api: ModelApi::Fake,
        usage: ModelUsage {
            input_tokens: 1,
            output_tokens: 1,
        },
        stop_reason: ModelStopReason::ToolUse,
        opaque_replay: "injected".into(),
    }
}

#[rstest]
fn interruption_rejects_late_work_before_model_completion(mut app: App) {
    let session = active_session(&app);

    majin::SubmitPrompt {
        session,
        text: "cancel me".into(),
    }
    .apply(app.world_mut());
    let turn = single_entity::<Turn>(&mut app);
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&ModelRequest>()
            .iter(app.world())
            .any(|request| request.status == WorkStatus::Running)
    });
    let (work, request) = app
        .world_mut()
        .query::<(Entity, &ModelRequest)>()
        .single(app.world())
        .unwrap();
    let request = request.clone();
    InterruptTurn { turn }.apply(app.world_mut());
    app.world_mut().write_message(ModelResult {
        session,
        turn,
        work,
        request_id: request.id,
        generation: request.generation,
        result: Err(ProviderFailure {
            message: "late response".into(),
        }),
    });
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&TurnCancelled>()
            .iter(app.world())
            .count()
            == 1
    });

    assert_eq!(
        app.world_mut()
            .query::<&TurnCancelled>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world_mut()
            .query::<&ModelResponse>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&ToolUse>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&TurnCompleted>()
            .iter(app.world())
            .count(),
        0
    );
}

#[rstest]
fn interruption_cancels_a_pending_tool_use(mut app: App) {
    let session = active_session(&app);

    majin::SubmitPrompt {
        session,
        text: "cancel tool".into(),
    }
    .apply(app.world_mut());
    let turn = single_entity::<Turn>(&mut app);
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&ToolUse>()
            .iter(app.world())
            .any(|tool_use| tool_use.turn == turn && tool_use.status == WorkStatus::Running)
    });
    let (work, tool_use) = app
        .world_mut()
        .query::<(Entity, &ToolUse)>()
        .single(app.world())
        .unwrap();
    let tool_use = tool_use.clone();
    InterruptTurn { turn }.apply(app.world_mut());
    app.world_mut().write_message(ToolResult {
        session,
        turn,
        work,
        tool_call_id: tool_use.id,
        generation: tool_use.generation,
        result: Ok("late tool output".into()),
    });
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&TurnCancelled>()
            .iter(app.world())
            .any(|outcome| outcome.turn == turn)
    });

    assert_eq!(
        app.world_mut()
            .query::<&TurnCancelled>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world_mut()
            .query::<&ToolOutcome>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&ModelRequest>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&TurnCompleted>()
            .iter(app.world())
            .count(),
        0
    );
}

#[rstest]
fn inconsistent_provider_output_fails_the_request_after_recording_response(mut app: App) {
    let session = active_session(&app);
    majin::SubmitPrompt {
        session,
        text: "wrong stop".into(),
    }
    .apply(app.world_mut());
    let turn = single_entity::<Turn>(&mut app);
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&ModelRequest>()
            .iter(app.world())
            .any(|request| request.status == WorkStatus::Running)
    });
    let (work, request) = app
        .world_mut()
        .query::<(Entity, &ModelRequest)>()
        .single(app.world())
        .unwrap();
    let request = request.clone();
    let mut output = tool_call_output("wrong stop");
    output.stop_reason = ModelStopReason::Complete;
    app.world_mut().write_message(ModelResult {
        session,
        turn,
        work,
        request_id: request.id,
        generation: request.generation,
        result: Ok(output),
    });
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .any(|failed| matches!(failed.failure, TurnFailure::Provider(_)))
    });

    assert_eq!(
        app.world().get::<ModelRequest>(work).unwrap().status,
        WorkStatus::Failed
    );
    assert_eq!(
        app.world_mut()
            .query::<&ModelResponse>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world_mut()
            .query::<&ToolUse>()
            .iter(app.world())
            .count(),
        0
    );
}

#[rstest]
fn initial_final_provider_output_fails_the_request_after_recording_response(mut app: App) {
    let session = active_session(&app);
    majin::SubmitPrompt {
        session,
        text: "wrong continuation".into(),
    }
    .apply(app.world_mut());
    let turn = single_entity::<Turn>(&mut app);
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&ModelRequest>()
            .iter(app.world())
            .any(|request| request.status == WorkStatus::Running)
    });
    let (work, request) = app
        .world_mut()
        .query::<(Entity, &ModelRequest)>()
        .single(app.world())
        .unwrap();
    let request = request.clone();
    app.world_mut().write_message(ModelResult {
        session,
        turn,
        work,
        request_id: request.id,
        generation: request.generation,
        result: Ok(ModelOutput {
            reply: majin::ModelReply::Final {
                text: "wrong continuation".into(),
            },
            assistant_content: Some("wrong continuation".into()),
            response_id: "wrong-continuation".into(),
            api: ModelApi::Fake,
            usage: ModelUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
            stop_reason: ModelStopReason::Complete,
            opaque_replay: "wrong-continuation".into(),
        }),
    });
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .any(|failed| matches!(failed.failure, TurnFailure::Provider(_)))
    });

    assert_eq!(
        app.world().get::<ModelRequest>(work).unwrap().status,
        WorkStatus::Failed
    );
    assert_eq!(
        app.world_mut()
            .query::<&ModelResponse>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world_mut()
            .query::<&TurnCompleted>()
            .iter(app.world())
            .count(),
        0
    );
}

#[rstest]
fn unavailable_tool_after_provider_success_keeps_model_response(mut app: App) {
    let session = active_session(&app);
    majin::SubmitPrompt {
        session,
        text: "remove tool".into(),
    }
    .apply(app.world_mut());
    let turn = single_entity::<Turn>(&mut app);
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&ModelRequest>()
            .iter(app.world())
            .any(|request| request.status == WorkStatus::Running)
    });
    let (work, request) = app
        .world_mut()
        .query::<(Entity, &ModelRequest)>()
        .single(app.world())
        .unwrap();
    let request = request.clone();
    let tool = single_entity::<ToolDefinition>(&mut app);
    app.world_mut().entity_mut(tool).remove::<ToolDefinition>();
    app.world_mut().write_message(ModelResult {
        session,
        turn,
        work,
        request_id: request.id,
        generation: request.generation,
        result: Ok(tool_call_output("remove tool")),
    });
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .any(|failed| matches!(failed.failure, TurnFailure::Provider(_)))
    });

    assert_eq!(
        app.world_mut()
            .query::<&ModelResponse>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world().get::<ModelRequest>(work).unwrap().status,
        WorkStatus::Failed
    );
}

#[derive(Debug, Clone, Copy)]
enum MissingCapability {
    Provider,
    Tool,
}

#[rstest]
#[case::provider(MissingCapability::Provider)]
#[case::tool(MissingCapability::Tool)]
fn missing_capability_fails_work(mut app: App, #[case] capability: MissingCapability) {
    let session = active_session(&app);
    match capability {
        MissingCapability::Provider => {
            let provider = single_entity::<Provider>(&mut app);
            app.world_mut().entity_mut(provider).remove::<Provider>();
            majin::SubmitPrompt {
                session,
                text: "missing provider".into(),
            }
            .apply(app.world_mut());
        }
        MissingCapability::Tool => {
            majin::SubmitPrompt {
                session,
                text: "missing tool".into(),
            }
            .apply(app.world_mut());
            update_until(&mut app, |app| {
                app.world_mut()
                    .query::<&ToolUse>()
                    .iter(app.world())
                    .next()
                    .is_some()
            });
            let tool = single_entity::<ToolDefinition>(&mut app);
            app.world_mut().entity_mut(tool).remove::<ToolDefinition>();
        }
    }
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .any(|failed| {
                matches!(
                    (capability, &failed.failure),
                    (MissingCapability::Provider, TurnFailure::Provider(_))
                        | (MissingCapability::Tool, TurnFailure::Tool(_))
                )
            })
    });
}

#[rstest]
fn final_reply_persists_text_without_assistant_content(mut app: App) {
    let session = active_session(&app);
    majin::SubmitPrompt {
        session,
        text: "final text".into(),
    }
    .apply(app.world_mut());
    let turn = single_entity::<Turn>(&mut app);
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<(Entity, &ToolUse)>()
            .iter(app.world())
            .any(|(_, tool_use)| tool_use.status == WorkStatus::Running)
    });
    let (tool_work, tool_use) = app
        .world_mut()
        .query::<(Entity, &ToolUse)>()
        .single(app.world())
        .unwrap();
    let tool_use = tool_use.clone();
    app.world_mut().write_message(ToolResult {
        session,
        turn,
        work: tool_work,
        tool_call_id: tool_use.id,
        generation: tool_use.generation,
        result: Ok("tool complete".into()),
    });
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&ModelRequest>()
            .iter(app.world())
            .any(|request| {
                request.previous_tool_use == Some(tool_work)
                    && request.status == WorkStatus::Running
            })
    });
    let (work, request) = app
        .world_mut()
        .query::<(Entity, &ModelRequest)>()
        .iter(app.world())
        .find(|(_, request)| request.previous_tool_use == Some(tool_work))
        .unwrap();
    let request = request.clone();
    app.world_mut().write_message(ModelResult {
        session,
        turn,
        work,
        request_id: request.id,
        generation: request.generation,
        result: Ok(ModelOutput {
            reply: majin::ModelReply::Final {
                text: "provided final".into(),
            },
            assistant_content: None,
            response_id: "provided-final".into(),
            api: ModelApi::Fake,
            usage: ModelUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
            stop_reason: ModelStopReason::Complete,
            opaque_replay: "provided-final".into(),
        }),
    });
    update_until(&mut app, |app| {
        app.world_mut()
            .query::<&TurnCompleted>()
            .iter(app.world())
            .any(|outcome| outcome.turn == turn)
    });

    assert!(
        app.world_mut()
            .query::<&AssistantMessage>()
            .iter(app.world())
            .any(|message| message.turn == turn && message.text == "provided final")
    );
}
