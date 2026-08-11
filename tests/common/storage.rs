use std::{
    fs,
    path::{Path, PathBuf},
};

use majin::SessionId;

pub fn session_log(root: &Path, id: SessionId) -> PathBuf {
    root.join(format!("session-{}.jsonl", id.0))
}

pub fn read_log(root: &Path, id: SessionId) -> String {
    fs::read_to_string(session_log(root, id)).expect("session log")
}
