use bevy::prelude::*;
use majin::{Agent, AgentTool, Model, Provider, ToolDefinition};

use super::runtime::single;

pub fn assert_capability_graph(app: &mut App) {
    let provider = single::<Provider>(app);
    let model = single::<Model>(app);
    let agent = single::<Agent>(app);
    let tool = single::<ToolDefinition>(app);
    let exposure = single::<AgentTool>(app);

    pretty_assertions::assert_eq!(app.world().get::<Model>(model).unwrap().provider, provider);
    pretty_assertions::assert_eq!(app.world().get::<Agent>(agent).unwrap().model, model);
    let exposure = app.world().get::<AgentTool>(exposure).unwrap();
    pretty_assertions::assert_eq!(exposure.agent, agent);
    pretty_assertions::assert_eq!(exposure.tool, tool);
}
