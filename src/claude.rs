//! Lectura ligera de las sesiones de Claude Code (JSONL) para listar sub agentes.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub fn sanitize(p: &str) -> String {
    p.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

pub fn session_path(config_dir: &Path, cwd: &Path, sid: &str) -> PathBuf {
    config_dir
        .join("projects")
        .join(sanitize(&cwd.to_string_lossy()))
        .join(format!("{sid}.jsonl"))
}

#[derive(Clone)]
pub struct Agent {
    pub id: String,
    pub kind: String,
    pub desc: String,
    pub done: bool,
}

/// Lee el archivo de sesión de forma incremental: solo lo nuevo desde la última vez.
#[derive(Default)]
pub struct Tracker {
    path: Option<PathBuf>,
    offset: u64,
    leftover: String,
    pub agents: Vec<Agent>,
}

impl Tracker {
    pub fn running(&self) -> usize {
        self.agents.iter().filter(|a| !a.done).count()
    }

    pub fn poll(&mut self, path: &Path) {
        if self.path.as_deref() != Some(path) {
            *self = Tracker { path: Some(path.to_path_buf()), ..Default::default() };
        }
        let Ok(meta) = std::fs::metadata(path) else { return };
        let len = meta.len();
        if len < self.offset {
            *self = Tracker { path: Some(path.to_path_buf()), ..Default::default() };
        }
        if len == self.offset {
            return;
        }
        let Ok(mut f) = File::open(path) else { return };
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut bytes = Vec::new();
        if f.take(len - self.offset).read_to_end(&mut bytes).is_err() {
            return;
        }
        self.offset = len;
        let mut data = std::mem::take(&mut self.leftover);
        data.push_str(&String::from_utf8_lossy(&bytes));
        let complete = data.ends_with('\n');
        let mut lines: Vec<&str> = data.split('\n').collect();
        if !complete {
            self.leftover = lines.pop().unwrap_or("").to_string();
        }
        for line in lines {
            if line.trim().is_empty() || !(line.contains("tool_use") || line.contains("tool_result")) {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let Some(items) = v["message"]["content"].as_array() else { continue };
            for it in items {
                match it["type"].as_str() {
                    Some("tool_use") => {
                        let name = it["name"].as_str().unwrap_or("");
                        if name == "Task" || name == "Agent" {
                            self.agents.push(Agent {
                                id: it["id"].as_str().unwrap_or("").to_string(),
                                kind: it["input"]["subagent_type"].as_str().unwrap_or("general").to_string(),
                                desc: it["input"]["description"].as_str().unwrap_or("").to_string(),
                                done: false,
                            });
                        }
                    }
                    Some("tool_result") => {
                        let id = it["tool_use_id"].as_str().unwrap_or("");
                        if let Some(a) = self.agents.iter_mut().find(|a| a.id == id) {
                            a.done = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
