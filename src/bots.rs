//! Bots líderes: carpeta de trabajo, instrucciones (CLAUDE.md), CLI `tc` e inbox de órdenes.

use crate::config::{self, Bot, Config};
use crate::i18n::{en, tr};
use crate::trf;
use crate::git;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Orden que el bot deja en ~/.tonycode/inbox y la app ejecuta.
#[derive(Serialize, Deserialize)]
pub struct Order {
    pub kind: String, // "delegate" | "send"
    pub bot: String,
    pub id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
}

/// Estado que la app publica para el bot en bots/<id>/status.json.
#[derive(Serialize, Deserialize, Default)]
pub struct Status {
    pub bot: String,
    pub updated: String,
    pub tasks: Vec<TaskStatus>,
    pub events: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TaskStatus {
    pub id: String,
    pub project: String,
    pub title: String,
    pub state: String,
    pub branch: String,
    pub cwd: String,
    pub agents_running: usize,
    pub last_lines: Vec<String>,
}

/// Prepara la carpeta del bot: CLAUDE.md, wrapper `tc` y permiso para usarlo.
pub fn prepare(cfg: &Config, bot: &Bot) -> std::io::Result<PathBuf> {
    let dir = bot.dir();
    std::fs::create_dir_all(dir.join(".claude"))?;
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("tonycode"));
    let tc = dir.join("tc");
    std::fs::write(&tc, format!("#!/bin/sh\nTONYCODE_BOT='{}' exec '{}' \"$@\"\n", bot.id, exe.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tc, std::fs::Permissions::from_mode(0o755))?;
    }
    let settings = dir.join(".claude").join("settings.local.json");
    if !settings.exists() {
        std::fs::write(&settings, "{\n  \"permissions\": {\n    \"allow\": [\"Bash(./tc:*)\"]\n  }\n}\n")?;
    }
    std::fs::write(dir.join("CLAUDE.md"), instructions(cfg, bot))?;
    Ok(dir)
}

fn instructions(cfg: &Config, bot: &Bot) -> String {
    let mut projects = String::new();
    for p in cfg.bot_projects(bot) {
        let path = config::expand(&p.path);
        let remote = git::run(&path, &["remote", "get-url", "origin"]).map(|s| s.trim().to_string()).unwrap_or_default();
        let remote = if remote.is_empty() { String::new() } else { format!(" · remote: {remote}") };
        projects.push_str(&trf!("- **{}** · {} · cuenta claude-{}{}\n", "- **{}** · {} · account claude-{}{}\n", p.name, p.path, p.account, remote));
    }
    if projects.is_empty() {
        projects.push_str(tr("- (sin proyectos asignados todavía)\n", "- (no projects assigned yet)\n"));
    }
    if en() {
        return instructions_en(bot, &projects);
    }
    let approve = if bot.allow_approve {
        "Puedes responder a prompts de permiso con `./tc send`, pero solo para comandos de lectura, tests o builds. Nunca apruebes push, merge, deploy, borrados ni migraciones: eso lo decide el usuario."
    } else {
        "No puedes aprobar permisos: si una tarea queda en estado `needs`, avisa al usuario para que la apruebe en tonycode (sección Te necesita)."
    };
    format!(
        r#"# {name}: lead bot de tonycode

Eres un lead que coordina trabajo entre varios proyectos (repos). Recibes tickets o pedidos del usuario,
decides a qué proyecto pertenece cada uno y delegas. No editas el código de los proyectos tú mismo:
cada tarea delegada corre en su propia sesión de Claude dentro del proyecto{wt}.

## Proyectos disponibles

{projects}
## Comandos (usa siempre ./tc)

- `./tc projects` lista los proyectos que puedes usar.
- `./tc delegate <proyecto> --title "<TICKET: título corto>" --prompt "<instrucciones>"` crea la tarea y la arranca. Para prompts largos usa `--prompt-file archivo.md`.
- `./tc status` muestra cada tarea: estado (running, idle, needs, exited), rama y últimas líneas.
- `./tc tail <task-id> [n]` muestra las últimas líneas de una tarea.
- `./tc send <task-id> "<mensaje>"` le escribe a la sesión de esa tarea (seguimiento, aclaraciones).

## Cómo trabajar

1. Por cada ticket identifica el proyecto. Si no es claro a cuál repo pertenece, pregunta antes de delegar.
2. Delega una tarea por ticket. El prompt debe ser autocontenido: ID del ticket, objetivo, contexto,
   criterios de aceptación y restricciones. Incluye siempre: "No hagas push, merge ni deploy sin que el usuario lo apruebe".
3. Revisa `./tc status` para dar seguimiento. Cuando una tarea quede `idle`, usa `./tc tail` para ver qué reportó.
4. {approve}
5. Resume al usuario: qué se delegó, dónde (proyecto y rama) y en qué estado va cada tarea.
"#,
        name = bot.name,
        wt = if bot.use_worktree { " y en su propia rama (worktree)" } else { "" },
        projects = projects,
        approve = approve,
    )
}

pub fn status_path(bot_id: &str) -> PathBuf {
    config::app_dir().join("bots").join(bot_id).join("status.json")
}

// ---------------- CLI ----------------

/// Devuelve Some(código) si los argumentos eran un comando del CLI.
pub fn cli(args: &[String]) -> Option<i32> {
    let cmd = args.get(1)?.as_str();
    if !matches!(cmd, "projects" | "delegate" | "send" | "status" | "tail" | "help" | "--help") {
        return None;
    }
    Some(match run_cli(cmd, &args[2..]) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("tc: {e}");
            1
        }
    })
}

fn flag(rest: &[String], name: &str) -> Option<String> {
    rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1).cloned())
}

fn write_order(o: &Order) -> Result<(), String> {
    let dir = config::inbox_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.tmp", o.id));
    std::fs::write(&tmp, serde_json::to_string(o).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(format!("{}-{}.json", o.kind, uuid::Uuid::new_v4()))).map_err(|e| e.to_string())
}

fn resolve_task(cfg: &Config, bot: &Bot, id: &str) -> Result<String, String> {
    if id.len() < 4 {
        return Err(tr("usa al menos 4 caracteres del task-id", "use at least 4 characters of the task-id").into());
    }
    let hits: Vec<String> = cfg
        .groups
        .iter()
        .flat_map(|g| g.projects.iter())
        .flat_map(|p| p.chats.iter())
        .filter(|c| c.bot.as_deref() == Some(bot.id.as_str()) && c.id.starts_with(id))
        .map(|c| c.id.clone())
        .collect();
    match hits.len() {
        1 => Ok(hits[0].clone()),
        0 => Err(trf!("no encontré la tarea {id}", "task {id} not found")),
        _ => Err(trf!("{id} es ambiguo, usa más caracteres", "{id} is ambiguous, use more characters")),
    }
}

fn read_status(bot: &Bot) -> Status {
    std::fs::read_to_string(status_path(&bot.id))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn run_cli(cmd: &str, rest: &[String]) -> Result<(), String> {
    if cmd == "help" || cmd == "--help" {
        println!("{}", tr("tc projects | delegate <proyecto> --title T --prompt P | status | tail <id> [n] | send <id> <texto>", "tc projects | delegate <project> --title T --prompt P | status | tail <id> [n] | send <id> <text>"));
        return Ok(());
    }
    let cfg = Config::load();
    crate::i18n::set_lang(&cfg.lang);
    let bot_id = std::env::var("TONYCODE_BOT")
        .map_err(|_| tr("falta TONYCODE_BOT (corre ./tc desde la carpeta del bot)", "TONYCODE_BOT is missing (run ./tc from the bot folder)").to_string())?;
    let bot = cfg.bot(&bot_id).ok_or(tr("el bot ya no existe en tonycode", "the bot no longer exists in tonycode"))?.clone();
    match cmd {
        "projects" => {
            for p in cfg.bot_projects(&bot) {
                println!("{}\tclaude-{}\t{}", p.name, p.account, p.path);
            }
        }
        "delegate" => {
            let name = rest.first().ok_or(tr("uso: tc delegate <proyecto> --title T --prompt P", "usage: tc delegate <project> --title T --prompt P"))?;
            let project = cfg
                .bot_projects(&bot)
                .into_iter()
                .find(|p| p.name.eq_ignore_ascii_case(name) || p.path == *name)
                .ok_or_else(|| trf!("el proyecto '{name}' no existe o no está asignado a este bot (ver ./tc projects)", "project '{name}' does not exist or is not assigned to this bot (see ./tc projects)"))?;
            let title = flag(rest, "--title").unwrap_or_else(|| tr("Tarea", "Task").into());
            let prompt = match (flag(rest, "--prompt"), flag(rest, "--prompt-file")) {
                (Some(p), _) => p,
                (None, Some(f)) => std::fs::read_to_string(&f).map_err(|e| trf!("no pude leer {f}: {e}", "could not read {f}: {e}"))?,
                _ => return Err(tr("falta --prompt o --prompt-file", "missing --prompt or --prompt-file").into()),
            };
            let id = uuid::Uuid::new_v4().to_string();
            write_order(&Order {
                kind: "delegate".into(),
                bot: bot.id.clone(),
                id: id.clone(),
                project: project.path.clone(),
                title: title.clone(),
                text: prompt,
            })?;
            println!("{}", trf!("Tarea {} creada en {} ({title}). Arranca en ~1 s; revisa con ./tc status", "Task {} created in {} ({title}). Starts in ~1 s; check with ./tc status", &id[..8], project.name));
        }
        "send" => {
            let id = rest.first().ok_or(tr("uso: tc send <task-id> <texto>", "usage: tc send <task-id> <text>"))?;
            let text = rest[1..].join(" ");
            if text.trim().is_empty() {
                return Err(tr("falta el texto", "missing text").into());
            }
            let full = resolve_task(&cfg, &bot, id)?;
            let state = read_status(&bot).tasks.into_iter().find(|t| t.id == full).map(|t| t.state).unwrap_or_default();
            if state == "needs" && !bot.allow_approve {
                return Err(trf!("la tarea {id} espera aprobación del usuario; avísale para que la apruebe en tonycode", "task {id} is waiting for user approval; tell the user to approve it in tonycode"));
            }
            write_order(&Order { kind: "send".into(), bot: bot.id.clone(), id: full, project: String::new(), title: String::new(), text })?;
            println!("{}", trf!("Enviado a {id}", "Sent to {id}"));
        }
        "status" => {
            let st = read_status(&bot);
            if st.tasks.is_empty() {
                println!("{}", tr("Sin tareas todavía.", "No tasks yet."));
            }
            for t in &st.tasks {
                println!("{}", trf!("[{}] {}  {} / {}  rama {}  · {} sub agentes", "[{}] {}  {} / {}  branch {}  · {} subagents", t.state, &t.id[..8], t.project, t.title, t.branch, t.agents_running));
                for l in t.last_lines.iter().rev().take(3).rev() {
                    println!("    {l}");
                }
            }
            if !st.events.is_empty() {
                println!("\n{}", tr("Eventos recientes:", "Recent events:"));
                for e in st.events.iter().rev().take(5) {
                    println!("  - {e}");
                }
            }
            println!("\n{}", trf!("(actualizado {})", "(updated {})", st.updated));
        }
        "tail" => {
            let id = rest.first().ok_or(tr("uso: tc tail <task-id> [n]", "usage: tc tail <task-id> [n]"))?;
            let n: usize = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(30);
            let full = resolve_task(&cfg, &bot, id)?;
            let st = read_status(&bot);
            let t = st.tasks.iter().find(|t| t.id == full).ok_or(tr("esa tarea aún no tiene estado (¿está arrancando?)", "that task has no state yet (is it starting?)"))?;
            println!("[{}] {} / {}", t.state, t.project, t.title);
            let skip = t.last_lines.len().saturating_sub(n);
            for l in &t.last_lines[skip..] {
                println!("{l}");
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn cwd_string(p: &Path) -> String {
    config::short(p)
}

fn instructions_en(bot: &Bot, projects: &str) -> String {
    let approve = if bot.allow_approve {
        "You may answer permission prompts with `./tc send`, but only for read-only commands, tests or builds. Never approve push, merge, deploy, deletions or migrations: the user decides those."
    } else {
        "You cannot approve permissions: if a task is in `needs` state, tell the user so they approve it in tonycode (Needs you section)."
    };
    format!(
        r#"# {name}: tonycode lead bot

You are a lead who coordinates work across several projects (repos). You receive tickets or requests
from the user, decide which project each one belongs to, and delegate. You do not edit the projects'
code yourself: each delegated task runs in its own Claude session inside the project{wt}.

Always reply to the user in English.

## Available projects

{projects}
## Commands (always use ./tc)

- `./tc projects` lists the projects you can use.
- `./tc delegate <project> --title "<TICKET: short title>" --prompt "<instructions>"` creates the task and starts it. For long prompts use `--prompt-file file.md`.
- `./tc status` shows each task: state (running, idle, needs, exited), branch and last lines.
- `./tc tail <task-id> [n]` shows the last lines of a task.
- `./tc send <task-id> "<message>"` writes to that task's session (follow-ups, clarifications).

## How to work

1. For each ticket identify the project. If it is unclear which repo it belongs to, ask before delegating.
2. Delegate one task per ticket. The prompt must be self-contained: ticket ID, goal, context,
   acceptance criteria and constraints. Always include: "Do not push, merge or deploy without the user's approval".
3. Check `./tc status` to follow up. When a task goes `idle`, use `./tc tail` to see what it reported.
4. {approve}
5. Summarize for the user: what was delegated, where (project and branch) and the state of each task.
"#,
        name = bot.name,
        wt = if bot.use_worktree { " and on its own branch (worktree)" } else { "" },
        projects = projects,
        approve = approve,
    )
}
