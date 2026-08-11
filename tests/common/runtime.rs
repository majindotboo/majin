use std::{
    path::PathBuf,
    thread::yield_now,
    time::{Duration, Instant},
};

use assert_fs::TempDir;
use bevy::{
    app::{AppExit, Main, MainScheduleOrder, Startup},
    ecs::system::Command,
    prelude::*,
};
use majin::{
    ActiveAgent, ActiveSession, Agent, ContextCamera, ContextEntry, HarnessPlugin, MajinPlugin,
    MajinSet, Model, ModelApi, ModelReply, ModelRequest, ModelResult, ModelStopReason, ModelUsage,
    PersistenceConfig, PersistentContextCamera, Session, SessionId, SubmitPrompt, ToolDefinition,
    ToolResult, ToolUse, TurnCompleted, WorkStatus, project_context,
};
use rstest::fixture;

pub struct Harness {
    pub app: App,
    pub session: Entity,
    pub agent: Entity,
    pub model: Entity,
    pub provider: Entity,
    pub tool: Entity,
}

impl Harness {
    pub fn disabled() -> Self {
        let mut app = App::new();
        app.add_plugins(HarnessPlugin);
        app.configure_sets(Update, (MajinSet::Dispatch, MajinSet::Apply).chain());

        let provider = app
            .world_mut()
            .spawn(majin::Provider {
                provider_id: majin::ProviderId(1),
            })
            .id();
        let model = app
            .world_mut()
            .spawn(Model {
                provider,
                model_id: "fake-model".into(),
            })
            .id();
        let tool = app
            .world_mut()
            .spawn(ToolDefinition {
                tool_id: majin::ToolId(1),
                name: "fake_tool".into(),
                description: "Temporary fake tool capability.".into(),
            })
            .id();
        let agent = app.world_mut().spawn(Agent { model }).id();
        app.world_mut().spawn(majin::AgentTool {
            agent,
            tool,
            order: 0,
        });
        let session = app
            .world_mut()
            .spawn(Session {
                id: SessionId(1),
                active_head: None,
            })
            .id();
        app.insert_resource(ActiveAgent(agent));
        app.insert_resource(ActiveSession(session));
        app.insert_resource(majin::HarnessReady);
        app.world_mut().spawn((
            ContextCamera {
                agent,
                session,
                head: None,
                budget: 4096,
            },
            PersistentContextCamera,
        ));

        Self {
            app,
            session,
            agent,
            model,
            provider,
            tool,
        }
    }

    pub fn full() -> Self {
        Self::with_config(PersistenceConfig::disabled())
    }

    pub fn persistent(path: PathBuf, debounce: Duration) -> Self {
        Self::with_config(PersistenceConfig {
            path,
            debounce,
            enabled: true,
        })
    }

    fn with_config(config: PersistenceConfig) -> Self {
        let persistence_enabled = config.enabled;
        let mut app = App::new();
        app.insert_resource(config);
        app.add_plugins(MajinPlugin);
        run_startup(&mut app);
        if persistence_enabled {
            app.world_mut().run_schedule(Update);
        }

        let session = app.world().resource::<ActiveSession>().0;
        let agent = app.world().resource::<ActiveAgent>().0;
        let model = app.world().get::<Agent>(agent).expect("active agent").model;
        let provider = app
            .world()
            .get::<Model>(model)
            .expect("active model")
            .provider;
        let tool = single::<ToolDefinition>(&mut app);

        Self {
            app,
            session,
            agent,
            model,
            provider,
            tool,
        }
    }

    pub fn submit(&mut self, text: impl Into<String>) -> Entity {
        SubmitPrompt {
            session: self.session,
            text: text.into(),
        }
        .apply(self.app.world_mut());
        self.app
            .world()
            .get::<Session>(self.session)
            .expect("test session")
            .active_head
            .expect("accepted prompt creates a turn")
    }

    pub fn complete(&mut self, turn: Entity) {
        update_until(&mut self.app, |app| {
            app.world_mut()
                .query::<&TurnCompleted>()
                .iter(app.world())
                .any(|outcome| outcome.turn == turn)
        });
    }

    pub fn flush_persistence(&mut self) {
        self.app.world_mut().write_message(AppExit::Success);
        self.app.update();
    }

    pub fn mark_completed(&mut self, turn: Entity) {
        let world = self.app.world_mut();
        let mut requests = world.query::<&mut ModelRequest>();
        for mut request in requests.iter_mut(world) {
            if request.turn == turn {
                request.status = WorkStatus::Succeeded;
            }
        }
        world.spawn(TurnCompleted {
            turn,
            generation: 0,
            sequence: majin::Sequence(0),
        });
    }

    pub fn complete_with_fake_results(&mut self, turn: Entity) {
        // Exercise result application without making every generated case wait on task polling.
        self.complete_one_with_fake_results(turn);
    }

    fn complete_one_with_fake_results(&mut self, turn: Entity) {
        let context_camera = {
            let world = self.app.world_mut();
            let mut cameras = world.query::<(Entity, &ContextCamera, &PersistentContextCamera)>();
            cameras
                .iter(world)
                .find(|(_, camera, _)| camera.agent == self.agent && camera.session == self.session)
                .map(|(entity, _, _)| entity)
                .expect("persistent context camera")
        };
        let input = project_context(self.app.world_mut(), context_camera)
            .expect("projected context")
            .entries
            .into_iter()
            .rev()
            .find_map(|entry| match entry {
                ContextEntry::User(text) => Some(text),
                _ => None,
            })
            .unwrap_or_default();
        let (work, request_id, generation) = {
            let world = self.app.world_mut();
            let mut requests = world.query::<(Entity, &ModelRequest)>();
            let (work, request) = requests
                .iter(world)
                .find(|(_, request)| request.turn == turn && request.previous_tool_use.is_none())
                .expect("initial model request");
            (work, request.id, request.generation)
        };
        self.app
            .world_mut()
            .get_mut::<ModelRequest>(work)
            .expect("initial model work")
            .status = WorkStatus::Running;
        let tool_id = self.tool_id();
        self.app.world_mut().write_message(ModelResult {
            session: self.session,
            turn,
            work,
            request_id,
            generation,
            result: Ok(tool_call_output(tool_id, &input, request_id.0)),
        });
        self.run_logic_update();

        let (tool_work, tool_call_id, generation) = {
            let world = self.app.world_mut();
            let mut tool_uses = world.query::<(Entity, &ToolUse)>();
            let (work, tool) = tool_uses
                .iter(world)
                .find(|(_, tool)| tool.turn == turn)
                .expect("tool work");
            (work, tool.id, tool.generation)
        };
        self.app
            .world_mut()
            .get_mut::<ToolUse>(tool_work)
            .expect("tool work")
            .status = WorkStatus::Running;
        self.app.world_mut().write_message(ToolResult {
            session: self.session,
            turn,
            work: tool_work,
            tool_call_id,
            generation,
            result: Ok(format!("Fake tool completed: {input}")),
        });
        self.run_logic_update();

        let (continuation_work, request_id, generation) = {
            let world = self.app.world_mut();
            let mut requests = world.query::<(Entity, &ModelRequest)>();
            let (work, request) = requests
                .iter(world)
                .find(|(_, request)| {
                    request.turn == turn && request.previous_tool_use == Some(tool_work)
                })
                .expect("continuation model request");
            (work, request.id, request.generation)
        };
        self.app
            .world_mut()
            .get_mut::<ModelRequest>(continuation_work)
            .expect("continuation model work")
            .status = WorkStatus::Running;
        self.app.world_mut().write_message(ModelResult {
            session: self.session,
            turn,
            work: continuation_work,
            request_id,
            generation,
            result: Ok(final_output(request_id.0)),
        });
        self.run_logic_update();
        assert!(
            self.app
                .world_mut()
                .query::<&TurnCompleted>()
                .iter(self.app.world())
                .any(|completed| completed.turn == turn)
        );
    }

    fn tool_id(&mut self) -> majin::ToolId {
        self.app
            .world()
            .get::<ToolDefinition>(self.tool)
            .expect("fake tool")
            .tool_id
    }

    fn run_logic_update(&mut self) {
        self.app.world_mut().run_schedule(Update);
    }

    pub fn select_branch(&mut self, head: Entity) {
        majin::SelectBranch {
            session: self.session,
            head,
        }
        .apply(self.app.world_mut());
    }

    pub fn add_session(&mut self, id: u64) -> Entity {
        self.app
            .world_mut()
            .spawn(Session {
                id: SessionId(id),
                active_head: None,
            })
            .id()
    }

    pub fn active_head(&self) -> Option<Entity> {
        self.app
            .world()
            .get::<Session>(self.session)
            .expect("test session")
            .active_head
    }

    pub fn count<T: Component>(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&T>()
            .iter(self.app.world())
            .count()
    }
}

fn run_startup(app: &mut App) {
    app.world_mut().run_schedule(Startup);

    let (labels, startup_labels) = {
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        (
            std::mem::take(&mut order.labels),
            std::mem::take(&mut order.startup_labels),
        )
    };
    app.world_mut().run_schedule(Main);
    let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
    order.labels = labels;
    order.startup_labels = startup_labels;
}

#[fixture]
pub fn harness() -> Harness {
    Harness::full()
}

#[fixture]
pub fn unstarted_app() -> App {
    let mut app = App::new();
    app.insert_resource(PersistenceConfig::disabled());
    app.add_plugins(MajinPlugin);
    app
}

#[fixture]
pub fn temp_dir() -> TempDir {
    TempDir::new().expect("temporary directory")
}

pub fn single<T: Component>(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<T>>()
        .single(app.world())
        .expect("one matching entity")
}

pub fn update_until(app: &mut App, mut predicate: impl FnMut(&mut App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        if predicate(app) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "condition did not complete before the five-second test deadline"
        );
        yield_now();
    }
}

fn tool_call_output(tool_id: majin::ToolId, input: &str, request_id: u64) -> majin::ModelOutput {
    majin::ModelOutput {
        reply: ModelReply::ToolCall {
            tool_id,
            input: input.into(),
        },
        assistant_content: Some("Calling fake_tool.".into()),
        response_id: format!("fake-response-{request_id}"),
        api: ModelApi::Fake,
        usage: ModelUsage {
            input_tokens: 0,
            output_tokens: 1,
        },
        stop_reason: ModelStopReason::ToolUse,
        opaque_replay: format!("fake-tool-call:{input}"),
    }
}

fn final_output(request_id: u64) -> majin::ModelOutput {
    majin::ModelOutput {
        reply: ModelReply::Final {
            text: "Fake harness completed the request.".into(),
        },
        assistant_content: Some("Fake harness completed the request.".into()),
        response_id: format!("fake-response-{request_id}"),
        api: ModelApi::Fake,
        usage: ModelUsage {
            input_tokens: 0,
            output_tokens: 1,
        },
        stop_reason: ModelStopReason::Complete,
        opaque_replay: "fake-final".into(),
    }
}
