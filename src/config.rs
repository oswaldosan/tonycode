use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub name: String,
    pub config_dir: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ChatCfg {
    pub id: String,
    pub name: String,
    pub session_id: String,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Bot que creó este chat (si fue delegado).
    #[serde(default)]
    pub bot: Option<String>,
    /// Prompt inicial pendiente (se envía al arrancar la sesión nueva).
    #[serde(default)]
    pub initial_prompt: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Bot {
    pub id: String,
    pub name: String,
    pub account: String,
    pub session_id: String,
    /// Rutas de proyectos que puede usar. Vacío = todos.
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default = "d_true")]
    pub use_worktree: bool,
    /// Si es true, `tc send` puede responder a prompts de permiso.
    #[serde(default)]
    pub allow_approve: bool,
}

impl Bot {
    pub fn dir(&self) -> PathBuf {
        app_dir().join("bots").join(&self.id)
    }
    pub fn allows(&self, project_path: &str) -> bool {
        self.projects.is_empty() || self.projects.iter().any(|p| p == project_path)
    }
}

pub fn inbox_dir() -> PathBuf {
    app_dir().join("inbox")
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Project {
    pub name: String,
    pub path: String,
    pub account: String,
    #[serde(default)]
    pub gh: Option<String>,
    /// None = automático (expandido si está seleccionado o tiene chats abiertos).
    #[serde(default)]
    pub expanded: Option<bool>,
    #[serde(default)]
    pub chats: Vec<ChatCfg>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub projects: Vec<Project>,
}

/// Datos opcionales por cuenta de GitHub (host de ~/.ssh/config).
#[derive(Serialize, Deserialize, Clone)]
pub struct GhIdentity {
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "d_claude")]
    pub claude_cmd: String,
    /// Lanza claude desde tu shell de login para heredar el PATH (nvm, bun, etc.).
    #[serde(default = "d_true")]
    pub login_shell: bool,
    #[serde(default = "d_true")]
    pub notify: bool,
    #[serde(default = "d_work")]
    pub default_account: String,
    /// Terminal: tema, fuente ("auto" o nombre), tamaño, interlineado y cursor (bar/block/underline)
    #[serde(default = "d_theme")]
    pub term_theme: String,
    #[serde(default = "d_auto")]
    pub term_font: String,
    #[serde(default = "d_size")]
    pub term_size: f32,
    #[serde(default = "d_line")]
    pub term_line: f32,
    #[serde(default = "d_cursor")]
    pub term_cursor: String,
    /// Modo voz: leer un resumen al terminar cada respuesta (con `say`).
    #[serde(default)]
    pub voice_enabled: bool,
    #[serde(default)]
    pub voice_name: String,
    #[serde(default = "d_rate")]
    pub voice_rate: u32,
    /// "es" o "en"
    #[serde(default = "d_lang")]
    pub lang: String,
    #[serde(default)]
    pub gh_active: Option<String>,
    #[serde(default)]
    pub last_chat: Option<String>,
    #[serde(default = "d_accounts")]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub gh_identities: Vec<GhIdentity>,
    #[serde(default)]
    pub bots: Vec<Bot>,
    #[serde(default = "d_groups")]
    pub groups: Vec<Group>,
}

fn d_claude() -> String {
    "claude".into()
}
fn d_true() -> bool {
    true
}
fn d_theme() -> String {
    "Catppuccin Mocha".into()
}
fn d_auto() -> String {
    "auto".into()
}
fn d_size() -> f32 {
    14.0
}
fn d_line() -> f32 {
    1.15
}
fn d_cursor() -> String {
    "bar".into()
}
fn d_rate() -> u32 {
    195
}
fn d_lang() -> String {
    "es".into()
}
fn d_work() -> String {
    "work".into()
}
fn d_accounts() -> Vec<Account> {
    vec![
        Account { name: "work".into(), config_dir: "~/.claude-work".into() },
        Account { name: "personal".into(), config_dir: "~/.claude-personal".into() },
    ]
}
fn d_groups() -> Vec<Group> {
    vec![
        Group { name: "Trabajo".into(), projects: vec![] },
        Group { name: "Freelance".into(), projects: vec![] },
        Group { name: "Personal".into(), projects: vec![] },
    ]
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

pub fn app_dir() -> PathBuf {
    home().join(".tonycode")
}

pub fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else if p == "~" {
        home()
    } else {
        PathBuf::from(p)
    }
}

pub fn short(p: &Path) -> String {
    let h = home();
    match p.strip_prefix(&h) {
        Ok(r) => format!("~/{}", r.display()),
        Err(_) => p.display().to_string(),
    }
}

impl Config {
    pub fn path() -> PathBuf {
        app_dir().join("tonycode.toml")
    }

    pub fn load() -> Config {
        let p = Self::path();
        if let Ok(s) = std::fs::read_to_string(&p) {
            match toml::from_str::<Config>(&s) {
                Ok(c) => return c,
                Err(e) => {
                    let _ = std::fs::copy(&p, p.with_extension("toml.bak"));
                    eprintln!("tonycode: config inválida ({e}), se respaldó y se usa una nueva");
                }
            }
        }
        toml::from_str("").unwrap_or_else(|_| Config {
            claude_cmd: d_claude(),
            login_shell: true,
            notify: true,
            default_account: d_work(),
            lang: d_lang(),
            voice_enabled: false,
            voice_name: String::new(),
            voice_rate: d_rate(),
            term_theme: d_theme(),
            term_font: d_auto(),
            term_size: d_size(),
            term_line: d_line(),
            term_cursor: d_cursor(),
            gh_active: None,
            last_chat: None,
            accounts: d_accounts(),
            gh_identities: vec![],
            bots: vec![],
            groups: d_groups(),
        })
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(app_dir());
        if let Ok(s) = toml::to_string_pretty(self) {
            let tmp = Self::path().with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, Self::path());
            }
        }
    }

    pub fn account_dir(&self, name: &str) -> PathBuf {
        self.accounts
            .iter()
            .find(|a| a.name == name)
            .map(|a| expand(&a.config_dir))
            .unwrap_or_else(|| home().join(format!(".claude-{name}")))
    }

    pub fn bot(&self, id: &str) -> Option<&Bot> {
        self.bots.iter().find(|b| b.id == id)
    }

    pub fn bot_projects(&self, bot: &Bot) -> Vec<&Project> {
        self.groups.iter().flat_map(|g| g.projects.iter()).filter(|p| bot.allows(&p.path)).collect()
    }

    pub fn find_project(&self, path: &str) -> Option<(usize, usize)> {
        for (gi, g) in self.groups.iter().enumerate() {
            for (pi, p) in g.projects.iter().enumerate() {
                if p.path == path {
                    return Some((gi, pi));
                }
            }
        }
        None
    }

    pub fn project(&self, path: &str) -> Option<&Project> {
        self.find_project(path).map(|(g, p)| &self.groups[g].projects[p])
    }

    pub fn project_mut(&mut self, path: &str) -> Option<&mut Project> {
        let (g, p) = self.find_project(path)?;
        Some(&mut self.groups[g].projects[p])
    }

    /// Devuelve (ruta del proyecto, chat) para un id de chat.
    pub fn find_chat(&self, id: &str) -> Option<(&Project, &ChatCfg)> {
        for g in &self.groups {
            for p in &g.projects {
                if let Some(c) = p.chats.iter().find(|c| c.id == id) {
                    return Some((p, c));
                }
            }
        }
        None
    }
}
