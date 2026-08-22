use bevy::{ecs::system::Command, prelude::*};
use majin::{
    AssistantMessage, InterruptTurn, ModelApi, ModelOutput, ModelReply, ModelRequest,
    ModelResponse, ModelResult, ModelStopReason, ModelUsage, Provider, ProviderFailure, ToolCallId,
    ToolDefinition, ToolOutcome, ToolResult, ToolUse, TurnCancelled, TurnCompleted, TurnFailed,
    TurnFailure, WorkStatus,
};
use pretty_assertions::assert_eq;
use rstest::{fixture, rstest};

pub mod common;

use common::{Harness, harness, update_until};

#[rstest]
#[case::model(WorkKind::Model)]
#[case::tool(WorkKind::Tool)]
fn stale_results_are_ignored_and_valid_results_apply_once(
    mut harness: Harness,
    #[case] kind: WorkKind,
) {
    let turn = harness.submit("stale result");
    match kind {
        WorkKind::Model => {
            update_until(&mut harness.app, |app| {
                app.world_mut()
                    .query::<&ModelRequest>()
                    .iter(app.world())
                    .any(|request| request.turn == turn && request.status == WorkStatus::Running)
            });
            let (work, request) = harness
                .app
                .world_mut()
                .query::<(Entity, &ModelRequest)>()
                .single(harness.app.world())
                .expect("pending model");
            let request = request.clone();
            harness.app.world_mut().write_message(ModelResult {
                session: harness.session,
                turn,
                work,
                request_id: request.id,
                generation: request.generation + 1,
                result: Ok(tool_call_output("stale", ModelStopReason::ToolUse)),
            });
            harness.app.world_mut().write_message(ModelResult {
                session: harness.session,
                turn,
                work,
                request_id: request.id,
                generation: request.generation,
                result: Ok(tool_call_output("valid", ModelStopReason::ToolUse)),
            });
            harness.app.update();
            assert_eq!(harness.count::<ModelResponse>(), 1);
            assert_eq!(harness.count::<ToolUse>(), 1);
            assert_eq!(harness.count::<TurnFailed>(), 0);
        }
        WorkKind::Tool => {
            update_until(&mut harness.app, |app| {
                app.world_mut()
                    .query::<&ToolUse>()
                    .iter(app.world())
                    .any(|tool| tool.turn == turn)
            });
            let (work, tool) = harness
                .app
                .world_mut()
                .query::<(Entity, &ToolUse)>()
                .single(harness.app.world())
                .expect("pending tool");
            let tool = tool.clone();
            harness.app.world_mut().write_message(ToolResult {
                session: harness.session,
                turn,
                work,
                tool_call_id: ToolCallId(tool.id.0 + 1),
                generation: tool.generation,
                result: Ok("stale".into()),
            });
            harness.app.world_mut().write_message(ToolResult {
                session: harness.session,
                turn,
                work,
                tool_call_id: tool.id,
                generation: tool.generation,
                result: Ok("valid".into()),
            });
            harness.app.update();
            assert_eq!(harness.count::<ToolOutcome>(), 1);
            assert_eq!(harness.count::<TurnFailed>(), 0);
        }
    }
}

#[rstest]
#[case::wrong_stop(InvalidModelOutput::ToolCallWithCompleteStop)]
#[case::final_before_tool(InvalidModelOutput::FinalBeforeToolCall)]
fn invalid_provider_outputs_record_the_response_then_fail(
    mut pending_model: PendingModel,
    #[case] output_kind: InvalidModelOutput,
) {
    let output = match output_kind {
        InvalidModelOutput::ToolCallWithCompleteStop => {
            tool_call_output("wrong stop", ModelStopReason::Complete)
        }
        InvalidModelOutput::FinalBeforeToolCall => final_output(Some("wrong continuation")),
    };
    pending_model
        .harness
        .app
        .world_mut()
        .write_message(ModelResult {
            session: pending_model.harness.session,
            turn: pending_model.turn,
            work: pending_model.work,
            request_id: pending_model.request.id,
            generation: pending_model.request.generation,
            result: Ok(output),
        });
    update_until(&mut pending_model.harness.app, |app| {
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .any(|failure| failure.turn == pending_model.turn)
    });

    assert_eq!(
        pending_model
            .harness
            .app
            .world()
            .get::<ModelRequest>(pending_model.work)
            .unwrap()
            .status,
        WorkStatus::Failed
    );
    assert_eq!(pending_model.harness.count::<ModelResponse>(), 1);
    assert_eq!(pending_model.harness.count::<ToolUse>(), 0);
    assert_eq!(pending_model.harness.count::<TurnCompleted>(), 0);
    assert!(
        pending_model
            .harness
            .app
            .world_mut()
            .query::<&TurnFailed>()
            .iter(pending_model.harness.app.world())
            .all(|failure| matches!(failure.failure, TurnFailure::Provider(_)))
    );
}

#[rstest]
fn provider_success_with_a_missing_tool_keeps_the_response(mut pending_model: PendingModel) {
    pending_model
        .harness
        .app
        .world_mut()
        .entity_mut(pending_model.harness.tool)
        .remove::<ToolDefinition>();
    pending_model
        .harness
        .app
        .world_mut()
        .write_message(ModelResult {
            session: pending_model.harness.session,
            turn: pending_model.turn,
            work: pending_model.work,
            request_id: pending_model.request.id,
            generation: pending_model.request.generation,
            result: Ok(tool_call_output("missing tool", ModelStopReason::ToolUse)),
        });
    update_until(&mut pending_model.harness.app, |app| {
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .any(|failure| failure.turn == pending_model.turn)
    });

    assert_eq!(pending_model.harness.count::<ModelResponse>(), 1);
    assert_eq!(pending_model.harness.count::<ToolUse>(), 0);
    assert_eq!(
        pending_model
            .harness
            .app
            .world()
            .get::<ModelRequest>(pending_model.work)
            .unwrap()
            .status,
        WorkStatus::Failed
    );
}

#[rstest]
#[case::provider(WorkKind::Model)]
#[case::tool(WorkKind::Tool)]
fn missing_capabilities_fail_the_current_work(mut harness: Harness, #[case] kind: WorkKind) {
    let turn = match kind {
        WorkKind::Model => {
            harness
                .app
                .world_mut()
                .entity_mut(harness.provider)
                .remove::<Provider>();
            harness.submit("missing provider")
        }
        WorkKind::Tool => {
            let turn = harness.submit("missing tool");
            update_until(&mut harness.app, |app| {
                app.world_mut()
                    .query::<&ToolUse>()
                    .iter(app.world())
                    .any(|tool| tool.turn == turn)
            });
            harness
                .app
                .world_mut()
                .entity_mut(harness.tool)
                .remove::<ToolDefinition>();
            turn
        }
    };
    update_until(&mut harness.app, |app| {
        app.world_mut()
            .query::<&TurnFailed>()
            .iter(app.world())
            .any(|failure| failure.turn == turn)
    });

    let failure = harness
        .app
        .world_mut()
        .query::<&TurnFailed>()
        .iter(harness.app.world())
        .find(|failure| failure.turn == turn)
        .expect("missing capability failure");
    assert_eq!(
        matches!(
            (&kind, &failure.failure),
            (WorkKind::Model, TurnFailure::Provider(_)) | (WorkKind::Tool, TurnFailure::Tool(_))
        ),
        true
    );
}

#[rstest]
#[case::model(WorkKind::Model)]
#[case::tool(WorkKind::Tool)]
fn interrupting_work_rejects_late_results_without_new_outcomes(
    mut harness: Harness,
    #[case] kind: WorkKind,
) {
    let turn = harness.submit("interrupt me");
    let (work, generation, tool_call_id) = match kind {
        WorkKind::Model => {
            update_until(&mut harness.app, |app| {
                app.world_mut()
                    .query::<&ModelRequest>()
                    .iter(app.world())
                    .any(|request| request.turn == turn && request.status == WorkStatus::Running)
            });
            let (work, request) = harness
                .app
                .world_mut()
                .query::<(Entity, &ModelRequest)>()
                .single(harness.app.world())
                .expect("pending model");
            (work, request.generation, None)
        }
        WorkKind::Tool => {
            update_until(&mut harness.app, |app| {
                app.world_mut()
                    .query::<&ToolUse>()
                    .iter(app.world())
                    .any(|tool| tool.turn == turn && tool.status == WorkStatus::Running)
            });
            let (work, tool) = harness
                .app
                .world_mut()
                .query::<(Entity, &ToolUse)>()
                .single(harness.app.world())
                .expect("pending tool");
            (work, tool.generation, Some(tool.id))
        }
    };
    InterruptTurn { turn }.apply(harness.app.world_mut());
    match kind {
        WorkKind::Model => {
            harness.app.world_mut().write_message(ModelResult {
                session: harness.session,
                turn,
                work,
                request_id: majin::ModelRequestId(1),
                generation,
                result: Err(ProviderFailure {
                    message: "late model".into(),
                }),
            });
        }
        WorkKind::Tool => {
            harness.app.world_mut().write_message(ToolResult {
                session: harness.session,
                turn,
                work,
                tool_call_id: tool_call_id.expect("tool id"),
                generation,
                result: Ok("late tool".into()),
            });
        }
    }
    update_until(&mut harness.app, |app| {
        app.world_mut()
            .query::<&TurnCancelled>()
            .iter(app.world())
            .any(|cancelled| cancelled.turn == turn)
    });

    assert_eq!(harness.count::<TurnCancelled>(), 1);
    assert_eq!(harness.count::<TurnFailed>(), 0);
    assert_eq!(harness.count::<TurnCompleted>(), 0);
    assert_eq!(harness.count::<ToolOutcome>(), 0);
    assert_eq!(
        harness.count::<ModelResponse>(),
        usize::from(matches!(kind, WorkKind::Tool))
    );
}

#[rstest]
fn final_reply_text_is_persisted_when_provider_omits_assistant_content(
    mut pending_tool: PendingTool,
) {
    pending_tool
        .harness
        .app
        .world_mut()
        .write_message(ToolResult {
            session: pending_tool.harness.session,
            turn: pending_tool.turn,
            work: pending_tool.work,
            tool_call_id: pending_tool.tool_use.id,
            generation: pending_tool.tool_use.generation,
            result: Ok("tool complete".into()),
        });
    update_until(&mut pending_tool.harness.app, |app| {
        app.world_mut()
            .query::<&ModelRequest>()
            .iter(app.world())
            .any(|request| {
                request.turn == pending_tool.turn
                    && request.previous_tool_use == Some(pending_tool.work)
                    && request.status == WorkStatus::Running
            })
    });
    let (work, request) = pending_tool
        .harness
        .app
        .world_mut()
        .query::<(Entity, &ModelRequest)>()
        .iter(pending_tool.harness.app.world())
        .find(|(_, request)| request.previous_tool_use == Some(pending_tool.work))
        .expect("final model request");
    let request = request.clone();
    pending_tool
        .harness
        .app
        .world_mut()
        .write_message(ModelResult {
            session: pending_tool.harness.session,
            turn: pending_tool.turn,
            work,
            request_id: request.id,
            generation: request.generation,
            result: Ok(final_output(None)),
        });
    pending_tool.harness.complete(pending_tool.turn);

    assert_eq!(
        pending_tool
            .harness
            .app
            .world_mut()
            .query::<&AssistantMessage>()
            .iter(pending_tool.harness.app.world())
            .filter(|message| message.turn == pending_tool.turn)
            .map(|message| message.text.clone())
            .collect::<Vec<_>>(),
        ["Calling fake_tool.".to_owned(), "provided final".to_owned()]
    );
}

#[derive(Debug, Clone, Copy)]
enum InvalidModelOutput {
    ToolCallWithCompleteStop,
    FinalBeforeToolCall,
}

#[derive(Debug, Clone, Copy)]
enum WorkKind {
    Model,
    Tool,
}

struct PendingModel {
    harness: Harness,
    turn: Entity,
    work: Entity,
    request: ModelRequest,
}

struct PendingTool {
    harness: Harness,
    turn: Entity,
    work: Entity,
    tool_use: ToolUse,
}

#[fixture]
fn pending_model(mut harness: Harness) -> PendingModel {
    let turn = harness.submit("pending model");
    update_until(&mut harness.app, |app| {
        app.world_mut()
            .query::<&ModelRequest>()
            .iter(app.world())
            .any(|request| request.turn == turn && request.status == WorkStatus::Running)
    });
    let (work, request) = {
        let world = harness.app.world_mut();
        let mut query = world.query::<(Entity, &ModelRequest)>();
        let (work, request) = query.single(world).expect("one pending model request");
        (work, request.clone())
    };
    PendingModel {
        harness,
        turn,
        work,
        request,
    }
}

#[fixture]
fn pending_tool(mut harness: Harness) -> PendingTool {
    let turn = harness.submit("pending tool");
    update_until(&mut harness.app, |app| {
        app.world_mut()
            .query::<&ToolUse>()
            .iter(app.world())
            .any(|tool| tool.turn == turn && tool.status == WorkStatus::Running)
    });
    let (work, tool_use) = {
        let world = harness.app.world_mut();
        let mut query = world.query::<(Entity, &ToolUse)>();
        let (work, tool_use) = query.single(world).expect("one pending tool use");
        (work, tool_use.clone())
    };
    PendingTool {
        harness,
        turn,
        work,
        tool_use,
    }
}

fn tool_call_output(input: &str, stop_reason: ModelStopReason) -> ModelOutput {
    ModelOutput {
        reply: ModelReply::ToolCall {
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
        stop_reason,
        opaque_replay: "injected".into(),
    }
}

fn final_output(assistant_content: Option<&str>) -> ModelOutput {
    ModelOutput {
        reply: ModelReply::Final {
            text: "provided final".into(),
        },
        assistant_content: assistant_content.map(str::to_owned),
        response_id: "provided-final".into(),
        api: ModelApi::Fake,
        usage: ModelUsage {
            input_tokens: 1,
            output_tokens: 1,
        },
        stop_reason: ModelStopReason::Complete,
        opaque_replay: "provided-final".into(),
    }
}
