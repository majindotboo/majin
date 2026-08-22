use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    time::{Duration, Instant},
};

use bevy::prelude::*;

use crate::{MajinSet, MajinStartupSet, PersistenceFailure, SessionId};

mod atomic;
use atomic::persist;
pub struct PersistencePlugin;

impl Plugin for PersistencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PersistenceConfig>()
            .init_resource::<PersistenceState>()
            .register_type::<Agent>()
            .register_type::<AgentTool>()
            .register_type::<AssistantMessage>()
            .register_type::<BranchSelection>()
            .register_type::<Compaction>()
            .register_type::<ContextCamera>()
            .register_type::<HarnessIds>()
            .register_type::<Model>()
            .register_type::<ModelChange>()
            .register_type::<ModelRequest>()
            .register_type::<ModelResponse>()
            .register_type::<PersistentContextCamera>()
            .register_type::<PersistenceFailure>()
            .register_type::<Provider>()
            .register_type::<Recovery>()
            .register_type::<Session>()
            .register_type::<ToolDefinition>()
            .register_type::<ToolOutcome>()
            .register_type::<ToolUse>()
            .register_type::<Turn>()
            .register_type::<TurnCancelled>()
            .register_type::<TurnCompleted>()
            .register_type::<TurnFailed>()
            .register_type::<TurnInterrupted>()
            .register_type::<UserMessage>()
            .add_systems(Update, persist.in_set(MajinSet::Persist));
    }
}

#[derive(Resource, Debug, Clone)]
pub struct PersistenceConfig {
    /// Directory containing one append-only JSONL history file per Session.
    pub path: PathBuf,
    pub debounce: Duration,
    pub enabled: bool,
}

impl PersistenceConfig {
    pub fn disabled() -> Self {
        Self {
            path: PathBuf::new(),
            debounce: Duration::ZERO,
            enabled: false,
        }
    }
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        default_path().map_or_else(Self::disabled, |path| Self {
            path,
            debounce: Duration::from_millis(250),
            enabled: true,
        })
    }
}

fn default_path() -> Option<PathBuf> {
    let home = {
        #[cfg(windows)]
        {
            env::var_os("USERPROFILE").or_else(|| {
                let drive = env::var_os("HOMEDRIVE")?;
                let path = env::var_os("HOMEPATH")?;
                Some(format!("{}{}", drive.to_string_lossy(), path.to_string_lossy()).into())
            })
        }
        #[cfg(not(windows))]
        {
            env::var_os("HOME")
        }
    };

    home.map(|home| PathBuf::from(home).join(".majin/sessions"))
}

#[derive(Resource, Default)]
struct PersistenceState {
    last_snapshot: Option<String>,
    dirty_since: Option<Instant>,
    blocked: bool,
    last_failure: Option<String>,
}
