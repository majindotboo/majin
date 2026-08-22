use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    time::{Duration, Instant},
};

use bevy::prelude::*;

use crate::{MajinSet, MajinStartupSet, PersistenceFailure, SessionId};

mod events;
mod log;
mod recovery;
mod replay;

pub struct PersistencePlugin;

impl Plugin for PersistencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PersistenceConfig>()
            .init_resource::<PersistenceState>()
            .add_systems(Startup, replay::hydrate.in_set(MajinStartupSet::Hydrate))
            .add_systems(Startup, recovery::recover.in_set(MajinStartupSet::Recover));
        if app.world().resource::<PersistenceConfig>().enabled {
            app.add_systems(Update, log::persist.in_set(MajinSet::Persist));
        }
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
pub(super) struct PersistenceState {
    pub(super) last_values: HashMap<String, String>,
    pub(super) next_ordinals: HashMap<SessionId, u64>,
    pub(super) dirty_since: Option<Instant>,
    pub(super) blocked: bool,
    pub(super) pending_failure: Option<String>,
    pub(super) last_failure: Option<String>,
}

pub(super) fn session_path(root: &std::path::Path, session: SessionId) -> PathBuf {
    root.join(format!("session-{}.jsonl", session.0))
}

pub(super) fn record_failure(world: &mut World, message: String) {
    let should_record = {
        let mut state = world.resource_mut::<PersistenceState>();
        if state.last_failure.as_deref() == Some(message.as_str()) {
            false
        } else {
            state.last_failure = Some(message.clone());
            true
        }
    };
    if should_record {
        let sequence = world
            .get_resource_mut::<crate::harness::HarnessIds>()
            .map(|mut ids| ids.sequence())
            .unwrap_or(crate::Sequence(0));
        world.spawn(PersistenceFailure { message, sequence });
    }
}
