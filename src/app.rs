use crate::config::{self, ChatCfg, Config, Project};
use crate::term::{self, Term};
use crate::updater::{self, Upd};
use crate::{bots, claude, git, ssh};
use std::sync::{Arc, Mutex};
use egui::{Align, Align2, Color32, FontId, Frame, Key, Layout, Margin, Modifiers, RichText, Sense, vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PANEL: Color32 = Color32::from_rgb(0x16, 0x17, 0x1A);
const HEAD: Color32 = Color32::from_rgb(0x11, 0x12, 0x14);
const FIELD: Color32 = Color32::from_rgb(0x0E, 0x0F, 0x11);
const LINE: Color32 = Color32::from_rgb(0x2A, 0x2D, 0x33);
const SEL: Color32 = Color32::from_rgb(0x23, 0x26, 0x2C);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE6);
const MUTED: Color32 = Color32::from_rgb(0x8A, 0x8F, 0x98);
const DIM: Color32 = Color32::from_rgb(0x5C, 0x61, 0x6B);
const WORK: Color32 = Color32::from_rgb(0x8A, 0xB4, 0xFF);
const PERS: Color32 = Color32::from_rgb(0xF0, 0xA0, 0x70);
const OTHER: Color32 = Color32::from_rgb(0xC3, 0xA6, 0xF0);
const RUN: Color32 = Color32::from_rgb(0xF0, 0xC0, 0x60);
const OK: Color32 = Color32::from_rgb(0x7F, 0xC8, 0xA9);
const NEED: Color32 = Color32::from_rgb(0xF0, 0x71, 0x78);

fn acct_color(name: &str) -> Color32 {
    match name {
        "work" => WORK,
        "personal" => PERS,
        _ => OTHER,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum St {
    Off,
    Idle,
    Running,
    Needs,
    Exited,
}

#[derive(Default)]
struct ChatRt {
    term: Option<Term>,
    tracker: claude::Tracker,
    st: Option<St>,
    review: bool,
    last_poll: Option<Instant>,
    branch: String,
    branch_at: Option<Instant>,
    err: Option<String>,
}

impl ChatRt {
    fn st(&self) -> St {
        self.st.unwrap_or(St::Off)
    }
    fn attention(&self) -> bool {
        self.st() == St::Needs || self.review
    }
}

fn arrow_btn(ui: &mut egui::Ui, up: bool, enabled: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    if enabled && resp.hovered() {
        ui.painter().rect_filled(r, 4.0, Color32::from_rgb(0x2C, 0x2F, 0x36));
    }
    let c = r.center();
    let col = if enabled { MUTED } else { Color32::from_gray(60) };
    let pts = if up {
        vec![c + vec2(-4.0, 2.5), c + vec2(4.0, 2.5), c + vec2(0.0, -3.0)]
    } else {
        vec![c + vec2(-4.0, -2.5), c + vec2(4.0, -2.5), c + vec2(0.0, 3.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
    enabled && resp.clicked()
}

fn dot(ui: &mut egui::Ui, c: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 3.5, c);
}

fn st_color(st: St, review: bool) -> Color32 {
    match st {
        St::Needs => NEED,
        _ if review => OK,
        St::Running => RUN,
        St::Idle => Color32::from_gray(120),
        St::Off | St::Exited => DIM,
    }
}

struct Editor {
    path: PathBuf,
    text: String,
    dirty: bool,
}

struct DiffView {
    status: Vec<String>,
    lines: Vec<(Color32, String)>,
}

enum Act {
    SelectProject(String),
    SelectChat(String),
    NewChat(String),
    Move(usize, usize, i32),
    MoveToGroup(String, usize),
    RemoveProject(String),
    AddProject(usize),
    AddGroup(String),
    DeleteGroup(usize),
    SetAccount(String, String),
    SetDefault(String),
    Restart(String),
    Stop(String),
    DeleteChat(String),
    Rename(String, String),
    SetGh(String),
    OpenFile(PathBuf),
    RefreshDiff,
    Commit,
    Revert,
    SelectBot(String),
    CreateBot,
    SetBotAccount(String, String),
    ToggleBotApprove(String),
    RestartBot(String),
    DeleteBot(String),
}

struct NewBot {
    name: String,
    account: String,
    projects: Vec<String>,
    worktree: bool,
    approve: bool,
}

pub struct App {
    cfg: Config,
    rt: HashMap<String, ChatRt>,
    project: Option<String>,
    chat: Option<String>,
    font: FontId,
    want_focus: bool,
    pal: bool,
    pal_q: String,
    pal_focus: bool,
    files: bool,
    diff_open: bool,
    gh_open: bool,
    new_wt: bool,
    browse: Option<PathBuf>,
    dir_cache: Option<(PathBuf, Vec<(String, bool)>)>,
    editor: Option<Editor>,
    diff: Option<DiffView>,
    commit_msg: String,
    confirm_revert: bool,
    rename: Option<(String, String)>,
    new_group: Option<String>,
    toast: Option<(String, Instant)>,
    gh_hosts: Vec<ssh::GhHost>,
    last_scan: Instant,
    term_input: Vec<egui::Event>,
    bot: Option<String>,
    new_bot: Option<NewBot>,
    last_inbox: Instant,
    last_status: Instant,
    pending_enter: Vec<(String, Instant)>,
    bot_events: HashMap<String, Vec<String>>,
    upd: Arc<Mutex<Upd>>,
    upd_open: bool,
    upd_term: Option<Term>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_style(&cc.egui_ctx);
        let cfg = Config::load();
        cfg.save();
        let last = cfg.last_chat.clone();
        let mut app = Self {
            cfg,
            rt: HashMap::new(),
            project: None,
            chat: None,
            font: FontId::monospace(13.0),
            want_focus: false,
            pal: false,
            pal_q: String::new(),
            pal_focus: false,
            files: false,
            diff_open: false,
            gh_open: false,
            new_wt: true,
            browse: None,
            dir_cache: None,
            editor: None,
            diff: None,
            commit_msg: String::new(),
            confirm_revert: false,
            rename: None,
            new_group: None,
            toast: None,
            gh_hosts: ssh::github_hosts(),
            last_scan: Instant::now(),
            term_input: Vec::new(),
            bot: None,
            new_bot: None,
            last_inbox: Instant::now(),
            last_status: Instant::now(),
            pending_enter: Vec::new(),
            bot_events: HashMap::new(),
            upd: Arc::new(Mutex::new(Upd::Unknown)),
            upd_open: false,
            upd_term: None,
        };
        updater::spawn_checker(app.upd.clone(), cc.egui_ctx.clone());
        if let Some(id) = last {
            if app.cfg.find_chat(&id).is_some() {
                app.apply(Act::SelectChat(id), &cc.egui_ctx);
            }
        }
        app
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn chat_cwd(p: &Project, c: &ChatCfg) -> PathBuf {
        match &c.worktree {
            Some(w) => config::expand(w),
            None => config::expand(&p.path),
        }
    }

    fn active_cwd(&self) -> Option<PathBuf> {
        let id = self.chat.as_ref()?;
        let (p, c) = self.cfg.find_chat(id)?;
        Some(Self::chat_cwd(p, c))
    }

    fn current_gh(&self) -> Option<String> {
        self.project
            .as_ref()
            .and_then(|p| self.cfg.project(p))
            .and_then(|p| p.gh.clone())
            .or_else(|| self.cfg.gh_active.clone())
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_claude(
        &mut self,
        key: &str,
        cwd: PathBuf,
        acct_dir: PathBuf,
        sid: String,
        prompt: Option<String>,
        extra_env: Vec<(String, String)>,
        ctx: &egui::Context,
    ) -> bool {
        let exists = claude::session_path(&acct_dir, &cwd, &sid).exists();
        let flag = if exists { "--resume" } else { "--session-id" };
        let prompt = if exists { None } else { prompt };
        let mut prompt_file = None;
        if let Some(pr) = &prompt {
            let dir = config::app_dir().join("prompts");
            let _ = std::fs::create_dir_all(&dir);
            let f = dir.join(format!("{key}.txt"));
            if std::fs::write(&f, pr).is_ok() {
                prompt_file = Some(f);
            }
        }
        let (prog, args) = if self.cfg.login_shell {
            let sh = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
            let mut line = format!("exec {} {} {}", self.cfg.claude_cmd, flag, sid);
            if let Some(f) = &prompt_file {
                line.push_str(&format!(" \"$(cat '{}')\"", f.display()));
            }
            (sh, vec!["-lic".to_string(), line])
        } else {
            let mut a = vec![flag.to_string(), sid];
            if let Some(pr) = prompt {
                a.push(pr);
            }
            (self.cfg.claude_cmd.clone(), a)
        };
        let mut env = vec![("CLAUDE_CONFIG_DIR".to_string(), acct_dir.to_string_lossy().to_string())];
        env.extend(extra_env);
        let rt = self.rt.entry(key.to_string()).or_default();
        if !cwd.exists() {
            rt.err = Some(format!("La carpeta no existe: {}", cwd.display()));
            return false;
        }
        match Term::spawn(&prog, &args, &cwd, &env, ctx.clone()) {
            Ok(t) => {
                rt.term = Some(t);
                rt.st = Some(St::Running);
                rt.err = None;
                true
            }
            Err(e) => {
                rt.err = Some(e);
                false
            }
        }
    }

    fn start_chat(&mut self, id: &str, ctx: &egui::Context) {
        let Some((p, c)) = self.cfg.find_chat(id) else { return };
        let cwd = Self::chat_cwd(p, c);
        let acct_dir = self.cfg.account_dir(&p.account);
        let sid = c.session_id.clone();
        let prompt = c.initial_prompt.clone();
        let had_prompt = prompt.is_some();
        if self.spawn_claude(id, cwd, acct_dir, sid, prompt, vec![], ctx) && had_prompt {
            for g in &mut self.cfg.groups {
                for p in &mut g.projects {
                    if let Some(c) = p.chats.iter_mut().find(|c| c.id == id) {
                        c.initial_prompt = None;
                    }
                }
            }
            self.cfg.save();
        }
    }

    fn start_bot(&mut self, id: &str, ctx: &egui::Context) {
        let Some(bot) = self.cfg.bot(id).cloned() else { return };
        match bots::prepare(&self.cfg, &bot) {
            Ok(dir) => {
                let acct = self.cfg.account_dir(&bot.account);
                self.spawn_claude(id, dir, acct, bot.session_id.clone(), None, vec![("TONYCODE_BOT".into(), bot.id.clone())], ctx);
            }
            Err(e) => self.rt.entry(id.to_string()).or_default().err = Some(format!("No pude preparar la carpeta del bot: {e}")),
        }
    }

    fn bot_event(&mut self, bot: &str, msg: String) {
        let v = self.bot_events.entry(bot.to_string()).or_default();
        let t = chrono_now();
        v.push(format!("{t} {msg}"));
        if v.len() > 30 {
            v.remove(0);
        }
    }

    /// Crea un chat en un proyecto (opcionalmente con worktree) y devuelve su id.
    fn create_chat(&mut self, path: &str, id: String, name: Option<String>, wt: bool, bot: Option<String>, prompt: Option<String>) -> Option<String> {
        let p = self.cfg.project(path)?;
        let n = p.chats.len() + 1;
        let pname = claude::sanitize(&p.name);
        let mut chat = ChatCfg {
            id: id.clone(),
            name: name.unwrap_or_else(|| format!("Chat {n}")),
            session_id: id.clone(),
            bot,
            initial_prompt: prompt,
            ..Default::default()
        };
        let root = config::expand(path);
        if wt {
            if git::is_repo(&root) {
                let slug = format!("chat-{n}-{}", &id[..4]);
                let wtp = config::app_dir().join("worktrees").join(&pname).join(&slug);
                let branch = format!("tonycode/{slug}");
                let _ = std::fs::create_dir_all(wtp.parent().unwrap());
                let wt_s = wtp.to_string_lossy().to_string();
                match git::run(&root, &["worktree", "add", "-b", &branch, &wt_s]) {
                    Ok(_) => {
                        chat.worktree = Some(config::short(&wtp));
                        chat.branch = Some(branch);
                    }
                    Err(e) => self.toast(format!("Sin worktree: {e}")),
                }
            } else {
                self.toast("La carpeta no es un repo git: chat sin worktree");
            }
        }
        self.cfg.project_mut(path)?.chats.push(chat);
        self.cfg.save();
        Some(id)
    }

    /// Lee las órdenes que dejan los bots en ~/.tonycode/inbox.
    fn process_inbox(&mut self, ctx: &egui::Context) {
        if self.last_inbox.elapsed() < Duration::from_millis(900) || self.cfg.bots.is_empty() {
            return;
        }
        self.last_inbox = Instant::now();
        let Ok(rd) = std::fs::read_dir(config::inbox_dir()) else { return };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "json").unwrap_or(false)).collect();
        files.sort();
        for f in files {
            let order = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<bots::Order>(&s).ok());
            let _ = std::fs::remove_file(&f);
            let Some(o) = order else { continue };
            let Some(bot) = self.cfg.bot(&o.bot).cloned() else { continue };
            match o.kind.as_str() {
                "delegate" => {
                    if !bot.allows(&o.project) {
                        self.bot_event(&bot.id, format!("Rechazada: {} no está asignado a este bot", o.project));
                        continue;
                    }
                    let pname = self.cfg.project(&o.project).map(|p| p.name.clone()).unwrap_or_default();
                    match self.create_chat(&o.project, o.id.clone(), Some(o.title.clone()), bot.use_worktree, Some(bot.id.clone()), Some(o.text)) {
                        Some(id) => {
                            self.start_chat(&id, ctx);
                            self.bot_event(&bot.id, format!("Delegada '{}' a {pname}", o.title));
                            self.toast(format!("{} delegó '{}' a {pname}", bot.name, o.title));
                        }
                        None => self.bot_event(&bot.id, format!("No pude crear la tarea '{}'", o.title)),
                    }
                }
                "send" => {
                    let owns = self.cfg.find_chat(&o.id).map(|(_, c)| c.bot.as_deref() == Some(bot.id.as_str())).unwrap_or(false);
                    if !owns {
                        self.bot_event(&bot.id, format!("send rechazado: la tarea {} no es de este bot", &o.id[..8]));
                        continue;
                    }
                    let needs = self.rt.get(&o.id).map(|r| r.st() == St::Needs).unwrap_or(false);
                    if needs && !bot.allow_approve {
                        self.bot_event(&bot.id, format!("Bloqueado: la tarea {} espera aprobación del usuario", &o.id[..8]));
                        continue;
                    }
                    if self.rt.get(&o.id).and_then(|r| r.term.as_ref()).is_none() {
                        self.start_chat(&o.id, ctx);
                    }
                    if let Some(t) = self.rt.get_mut(&o.id).and_then(|r| r.term.as_mut()) {
                        t.write(o.text.as_bytes());
                        self.pending_enter.push((o.id.clone(), Instant::now() + Duration::from_millis(250)));
                        self.bot_event(&bot.id, format!("Mensaje enviado a {}", &o.id[..8]));
                    }
                }
                _ => {}
            }
        }
    }

    /// Publica bots/<id>/status.json para que el bot vea sus tareas.
    fn write_bot_status(&mut self) {
        let now = Instant::now();
        let due: Vec<(String, Instant)> = self.pending_enter.iter().filter(|(_, t)| *t <= now).cloned().collect();
        self.pending_enter.retain(|(_, t)| *t > now);
        for (id, _) in due {
            if let Some(t) = self.rt.get_mut(&id).and_then(|r| r.term.as_mut()) {
                t.write(b"\r");
            }
        }
        if self.last_status.elapsed() < Duration::from_secs(2) || self.cfg.bots.is_empty() {
            return;
        }
        self.last_status = Instant::now();
        for bot in self.cfg.bots.clone() {
            if !bot.dir().exists() {
                continue;
            }
            let mut tasks = Vec::new();
            for g in &self.cfg.groups {
                for p in &g.projects {
                    for c in p.chats.iter().filter(|c| c.bot.as_deref() == Some(bot.id.as_str())) {
                        let cwd = Self::chat_cwd(p, c);
                        let rt = self.rt.entry(c.id.clone()).or_default();
                        let path = claude::session_path(&self.cfg.account_dir(&p.account), &cwd, &c.session_id);
                        rt.tracker.poll(&path);
                        let state = match rt.st() {
                            St::Running => "running",
                            St::Idle => "idle",
                            St::Needs => "needs",
                            St::Exited => "exited",
                            St::Off => "off",
                        };
                        let last_lines: Vec<String> = rt
                            .term
                            .as_ref()
                            .map(|t| t.screen_text())
                            .unwrap_or_default()
                            .lines()
                            .map(|l| l.trim_end().to_string())
                            .filter(|l| !l.trim().is_empty())
                            .collect();
                        let skip = last_lines.len().saturating_sub(40);
                        tasks.push(bots::TaskStatus {
                            id: c.id.clone(),
                            project: p.name.clone(),
                            title: c.name.clone(),
                            state: state.into(),
                            branch: c.branch.clone().unwrap_or_default(),
                            cwd: bots::cwd_string(&cwd),
                            agents_running: rt.tracker.running(),
                            last_lines: last_lines[skip..].to_vec(),
                        });
                    }
                }
            }
            let st = bots::Status {
                bot: bot.name.clone(),
                updated: chrono_now(),
                tasks,
                events: self.bot_events.get(&bot.id).cloned().unwrap_or_default(),
            };
            if let Ok(js) = serde_json::to_string_pretty(&st) {
                let path = bots::status_path(&bot.id);
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, js).is_ok() {
                    let _ = std::fs::rename(tmp, path);
                }
            }
        }
    }

    /// Revisa estados de todos los chats (barato: cada 400 ms).
    fn scan(&mut self, ctx: &egui::Context) {
        if self.last_scan.elapsed() < Duration::from_millis(400) {
            return;
        }
        self.last_scan = Instant::now();
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
        let mut notes: Vec<String> = Vec::new();
        for (id, rt) in self.rt.iter_mut() {
            let Some(t) = &rt.term else { continue };
            if !t.is_alive() {
                rt.st = Some(St::Exited);
                continue;
            }
            let text = t.screen_text();
            let bell = t.take_bell();
            let needs = text.contains("Do you want")
                || text.contains("Would you like to proceed")
                || text.contains("❯ 1. Yes");
            let running = t.idle_secs() < 2.5;
            let new = if needs { St::Needs } else if running { St::Running } else { St::Idle };
            let active = self.chat.as_deref() == Some(id.as_str()) || self.bot.as_deref() == Some(id.as_str());
            let old = rt.st();
            let name = self
                .cfg
                .find_chat(id)
                .map(|(p, c)| format!("{} / {}", p.name, c.name))
                .or_else(|| self.cfg.bot(id).map(|b| b.name.clone()))
                .unwrap_or_default();
            if active && focused {
                rt.review = false;
            } else {
                if old == St::Running && new == St::Idle && t.started.elapsed().as_secs() > 8 {
                    rt.review = true;
                    notes.push(format!("Terminó: {name}"));
                }
                if new == St::Needs && old != St::Needs {
                    notes.push(format!("Pide permiso: {name}"));
                }
                if bell && new != St::Needs {
                    rt.review = true;
                }
            }
            rt.st = Some(new);
        }
        if self.cfg.notify && !focused {
            for n in notes {
                notify(&n);
            }
        }
        // sub agentes y rama del chat activo
        if let Some(id) = self.chat.clone() {
            if let Some((p, c)) = self.cfg.find_chat(&id) {
                let cwd = Self::chat_cwd(p, c);
                let path = claude::session_path(&self.cfg.account_dir(&p.account), &cwd, &c.session_id);
                let rt = self.rt.entry(id).or_default();
                if rt.last_poll.map(|t| t.elapsed().as_secs_f32() > 1.5).unwrap_or(true) {
                    rt.last_poll = Some(Instant::now());
                    rt.tracker.poll(&path);
                }
                if rt.branch_at.map(|t| t.elapsed().as_secs() > 5).unwrap_or(true) {
                    rt.branch_at = Some(Instant::now());
                    rt.branch = git::branch(&cwd);
                }
            }
        }
    }

    /// El teclado va a la terminal si hay chat activo y ningún campo de texto tiene el foco.
    fn term_owns_keys(&self, ctx: &egui::Context) -> bool {
        (self.chat.is_some() || self.bot.is_some())
            && !self.pal
            && self.new_bot.is_none()
            && self.rename.is_none()
            && !self.confirm_revert
            && self.new_group.is_none()
            && ctx.memory(|m| m.focused().is_none())
    }

    fn grab_term_input(&mut self, ctx: &egui::Context) {
        if !self.term_owns_keys(ctx) {
            return;
        }
        let taken: Vec<egui::Event> = ctx.input_mut(|i| {
            let mut keep = Vec::new();
            let mut take = Vec::new();
            for e in i.events.drain(..) {
                match &e {
                    egui::Event::Text(_) | egui::Event::Paste(_) => take.push(e),
                    #[cfg(not(target_os = "macos"))]
                    egui::Event::Copy | egui::Event::Cut => take.push(e),
                    egui::Event::Key { modifiers, .. } if !modifiers.mac_cmd => take.push(e),
                    _ => keep.push(e),
                }
            }
            i.events = keep;
            take
        });
        self.term_input = taken;
    }

    fn shortcuts(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::K)) {
            self.pal = !self.pal;
            self.pal_q.clear();
            self.pal_focus = true;
        }
        if self.gh_open && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.gh_open = false;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save_editor();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::T)) {
            if let Some(p) = self.project.clone() {
                acts.push(Act::NewChat(p));
            }
        }
    }

    fn save_editor(&mut self) {
        if let Some(ed) = &mut self.editor {
            match std::fs::write(&ed.path, &ed.text) {
                Ok(_) => {
                    ed.dirty = false;
                    let n = ed.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    self.toast(format!("Guardado {n}"));
                }
                Err(e) => self.toast(format!("No se pudo guardar: {e}")),
            }
        }
    }

    fn apply(&mut self, act: Act, ctx: &egui::Context) {
        match act {
            Act::SelectProject(path) => {
                self.project = Some(path.clone());
                let first = self.cfg.project(&path).and_then(|p| p.chats.first().map(|c| c.id.clone()));
                match first {
                    Some(id) => self.apply(Act::SelectChat(id), ctx),
                    None => {
                        let keep = self.new_wt;
                        self.new_wt = false;
                        self.apply(Act::NewChat(path), ctx);
                        self.new_wt = keep;
                    }
                }
            }
            Act::SelectChat(id) => {
                let Some((p, _)) = self.cfg.find_chat(&id) else { return };
                let ppath = p.path.clone();
                if self.chat.as_deref() != Some(&id) {
                    self.browse = None;
                    self.dir_cache = None;
                    self.diff = None;
                    if self.diff_open {
                        self.chat = Some(id.clone());
                        self.apply(Act::RefreshDiff, ctx);
                    }
                }
                self.project = Some(ppath);
                self.chat = Some(id.clone());
                self.bot = None;
                self.want_focus = true;
                if self.cfg.last_chat.as_deref() != Some(&id) {
                    self.cfg.last_chat = Some(id.clone());
                    self.cfg.save();
                }
                let rt = self.rt.entry(id.clone()).or_default();
                rt.review = false;
                if rt.term.is_none() && rt.st() != St::Exited && rt.err.is_none() {
                    self.start_chat(&id, ctx);
                }
            }
            Act::NewChat(path) => {
                let id = uuid::Uuid::new_v4().to_string();
                if let Some(id) = self.create_chat(&path, id, None, self.new_wt, None, None) {
                    self.apply(Act::SelectChat(id), ctx);
                }
            }
            Act::Move(gi, pi, d) => {
                let len = self.cfg.groups[gi].projects.len() as i32;
                let j = pi as i32 + d;
                if j >= 0 && j < len {
                    self.cfg.groups[gi].projects.swap(pi, j as usize);
                    self.cfg.save();
                }
            }
            Act::MoveToGroup(path, gj) => {
                if let Some((gi, pi)) = self.cfg.find_project(&path) {
                    let p = self.cfg.groups[gi].projects.remove(pi);
                    self.cfg.groups[gj].projects.push(p);
                    self.cfg.save();
                }
            }
            Act::RemoveProject(path) => {
                if let Some((gi, pi)) = self.cfg.find_project(&path) {
                    let p = self.cfg.groups[gi].projects.remove(pi);
                    for c in &p.chats {
                        self.rt.remove(&c.id);
                        if self.chat.as_deref() == Some(&c.id) {
                            self.chat = None;
                        }
                    }
                    if self.project.as_deref() == Some(&path) {
                        self.project = None;
                    }
                    self.cfg.save();
                }
            }
            Act::AddProject(gi) => {
                if let Some(dir) = rfd::FileDialog::new().set_title("Elige la carpeta del proyecto").pick_folder() {
                    let path = config::short(&dir);
                    if self.cfg.find_project(&path).is_some() {
                        self.toast("Ese proyecto ya está agregado");
                        return;
                    }
                    let name = dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(path.clone());
                    if self.cfg.groups.is_empty() {
                        self.cfg.groups.push(config::Group { name: "Proyectos".into(), projects: vec![] });
                    }
                    let gi = gi.min(self.cfg.groups.len() - 1);
                    self.cfg.groups[gi].projects.push(Project {
                        name,
                        path: path.clone(),
                        account: self.cfg.default_account.clone(),
                        gh: None,
                        chats: vec![],
                    });
                    self.cfg.save();
                    self.project = Some(path);
                }
            }
            Act::AddGroup(name) => {
                if !name.trim().is_empty() {
                    self.cfg.groups.push(config::Group { name: name.trim().into(), projects: vec![] });
                    self.cfg.save();
                }
            }
            Act::DeleteGroup(gi) => {
                if self.cfg.groups.get(gi).map(|g| g.projects.is_empty()).unwrap_or(false) {
                    self.cfg.groups.remove(gi);
                    self.cfg.save();
                }
            }
            Act::SetAccount(path, a) => {
                if let Some(p) = self.cfg.project_mut(&path) {
                    if p.account != a {
                        p.account = a.clone();
                        self.cfg.save();
                        self.toast(format!("Los chats usarán claude-{a}. Reinicia el chat para aplicarlo."));
                    }
                }
            }
            Act::SetDefault(a) => {
                self.cfg.default_account = a;
                self.cfg.save();
            }
            Act::Restart(id) => {
                if let Some(rt) = self.rt.get_mut(&id) {
                    rt.term = None;
                    rt.err = None;
                    rt.st = None;
                }
                self.start_chat(&id, ctx);
                self.want_focus = true;
            }
            Act::Stop(id) => {
                if let Some(rt) = self.rt.get_mut(&id) {
                    rt.term = None;
                    rt.st = Some(St::Off);
                }
            }
            Act::DeleteChat(id) => {
                self.rt.remove(&id);
                let mut wt = None;
                for g in &mut self.cfg.groups {
                    for p in &mut g.projects {
                        if let Some(i) = p.chats.iter().position(|c| c.id == id) {
                            wt = p.chats.remove(i).worktree;
                        }
                    }
                }
                if self.chat.as_deref() == Some(&id) {
                    self.chat = None;
                }
                self.cfg.save();
                if let Some(w) = wt {
                    self.toast(format!("Chat eliminado. El worktree sigue en {w}"));
                }
            }
            Act::Rename(id, name) => {
                for g in &mut self.cfg.groups {
                    for p in &mut g.projects {
                        if let Some(c) = p.chats.iter_mut().find(|c| c.id == id) {
                            c.name = name.clone();
                        }
                    }
                }
                self.cfg.save();
            }
            Act::SetGh(alias) => {
                self.cfg.gh_active = Some(alias.clone());
                let host = self.gh_hosts.iter().find(|h| h.alias == alias).cloned();
                let mut applied = String::new();
                if let Some(path) = self.project.clone() {
                    if let Some(p) = self.cfg.project_mut(&path) {
                        p.gh = Some(alias.clone());
                    }
                    let root = config::expand(&path);
                    if let (Some(h), true) = (&host, git::is_repo(&root)) {
                        if !h.key.is_empty() {
                            let key = config::expand(&h.key);
                            let cmd = format!("ssh -i {} -o IdentitiesOnly=yes", key.display());
                            let _ = git::run(&root, &["config", "core.sshCommand", &cmd]);
                        }
                        if let Some(id) = self.cfg.gh_identities.iter().find(|i| i.host == alias).cloned() {
                            if let Some(u) = id.user {
                                let _ = git::run(&root, &["config", "user.name", &u]);
                            }
                            if let Some(e) = id.email {
                                let _ = git::run(&root, &["config", "user.email", &e]);
                            }
                        }
                        applied = format!(" y aplicada en {}", path);
                    }
                }
                self.cfg.save();
                let label = host.map(|h| h.label()).unwrap_or(alias);
                self.toast(format!("GitHub: {label} activa{applied}"));
            }
            Act::OpenFile(path) => {
                if self.editor.as_ref().map(|e| e.dirty).unwrap_or(false) {
                    self.toast("Guarda (Cmd+S) o descarta el archivo actual primero");
                    return;
                }
                match std::fs::metadata(&path) {
                    Ok(m) if m.len() > 2_000_000 => self.toast("Archivo muy grande para el editor"),
                    _ => match std::fs::read_to_string(&path) {
                        Ok(text) => self.editor = Some(Editor { path, text, dirty: false }),
                        Err(_) => self.toast("No es un archivo de texto"),
                    },
                }
            }
            Act::RefreshDiff => {
                let Some(cwd) = self.active_cwd() else { return };
                let status: Vec<String> = git::run(&cwd, &["status", "--porcelain"])
                    .unwrap_or_default()
                    .lines()
                    .map(|s| s.to_string())
                    .collect();
                let raw = git::run(&cwd, &["diff", "HEAD"]).or_else(|_| git::run(&cwd, &["diff"])).unwrap_or_default();
                let lines = raw
                    .lines()
                    .take(5000)
                    .map(|l| {
                        let c = if l.starts_with("diff --git") {
                            TEXT
                        } else if l.starts_with("+++") || l.starts_with("---") || l.starts_with("index ") {
                            DIM
                        } else if l.starts_with('+') {
                            Color32::from_rgb(0xA6, 0xE3, 0xB8)
                        } else if l.starts_with('-') {
                            Color32::from_rgb(0xF4, 0xA3, 0xAA)
                        } else if l.starts_with("@@") {
                            Color32::from_rgb(0x6E, 0x8B, 0xC4)
                        } else {
                            Color32::from_gray(150)
                        };
                        (c, l.to_string())
                    })
                    .collect();
                self.diff = Some(DiffView { status, lines });
            }
            Act::Commit => {
                let Some(cwd) = self.active_cwd() else { return };
                let msg = self.commit_msg.trim().to_string();
                let r = git::run(&cwd, &["add", "-A"]).and_then(|_| git::run(&cwd, &["commit", "-m", &msg]));
                match r {
                    Ok(_) => {
                        self.commit_msg.clear();
                        self.toast("Commit creado");
                    }
                    Err(e) => self.toast(format!("Commit falló: {e}")),
                }
                self.apply(Act::RefreshDiff, ctx);
            }
            Act::SelectBot(id) => {
                self.bot = Some(id.clone());
                self.chat = None;
                self.files = false;
                self.diff_open = false;
                self.want_focus = true;
                let rt = self.rt.entry(id.clone()).or_default();
                rt.review = false;
                if rt.term.is_none() && rt.st() != St::Exited && rt.err.is_none() {
                    self.start_bot(&id, ctx);
                }
            }
            Act::CreateBot => {
                let Some(nb) = self.new_bot.take() else { return };
                let id = uuid::Uuid::new_v4().to_string();
                self.cfg.bots.push(config::Bot {
                    id: id.clone(),
                    name: if nb.name.trim().is_empty() { "Lead Bot".into() } else { nb.name.trim().into() },
                    account: nb.account,
                    session_id: id.clone(),
                    projects: nb.projects,
                    use_worktree: nb.worktree,
                    allow_approve: nb.approve,
                });
                self.cfg.save();
                self.apply(Act::SelectBot(id), ctx);
            }
            Act::SetBotAccount(id, a) => {
                if let Some(b) = self.cfg.bots.iter_mut().find(|b| b.id == id) {
                    if b.account != a {
                        b.account = a.clone();
                        self.cfg.save();
                        self.toast(format!("El bot usará claude-{a}. Reinícialo para aplicarlo."));
                    }
                }
            }
            Act::ToggleBotApprove(id) => {
                if let Some(b) = self.cfg.bots.iter_mut().find(|b| b.id == id) {
                    b.allow_approve = !b.allow_approve;
                    let on = b.allow_approve;
                    self.cfg.save();
                    self.toast(if on { "El bot puede responder prompts de permiso (reinícialo)" } else { "El bot ya no puede aprobar permisos (reinícialo)" });
                }
            }
            Act::RestartBot(id) => {
                if let Some(rt) = self.rt.get_mut(&id) {
                    rt.term = None;
                    rt.err = None;
                    rt.st = None;
                }
                self.start_bot(&id, ctx);
                self.want_focus = true;
            }
            Act::DeleteBot(id) => {
                self.rt.remove(&id);
                self.cfg.bots.retain(|b| b.id != id);
                if self.bot.as_deref() == Some(&id) {
                    self.bot = None;
                }
                self.cfg.save();
                self.toast("Bot eliminado (sus tareas siguen en cada proyecto)");
            }
            Act::Revert => {
                let Some(cwd) = self.active_cwd() else { return };
                match git::run(&cwd, &["restore", "--staged", "--worktree", "."]) {
                    Ok(_) => self.toast("Cambios revertidos (los archivos nuevos sin seguimiento se conservan)"),
                    Err(e) => self.toast(format!("No se pudo revertir: {e}")),
                }
                self.apply(Act::RefreshDiff, ctx);
            }
        }
    }

    // ---------- UI ----------

    fn top_bar(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::TopBottomPanel::top("top")
            .exact_height(40.0)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(12, 6)).stroke(egui::Stroke::new(1.0_f32, LINE)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("tonycode").strong().color(TEXT));
                    let crumb = match self.chat.as_ref().and_then(|id| self.cfg.find_chat(id)) {
                        Some((p, c)) => format!("{}  /  {}", p.name, c.name),
                        None => self.bot.as_ref().and_then(|b| self.cfg.bot(b)).map(|b| format!("Bot  /  {}", b.name)).unwrap_or_default(),
                    };
                    ui.label(RichText::new(crumb).color(MUTED).small());
                    ui.add_space(24.0);
                    let b = egui::Button::new(RichText::new("Ir a proyecto o chat                Cmd+K").color(DIM).small())
                        .fill(FIELD)
                        .min_size(vec2(300.0, 26.0));
                    if ui.add(b).clicked() {
                        self.pal = true;
                        self.pal_q.clear();
                        self.pal_focus = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for a in self.cfg.accounts.iter().rev() {
                            let on = self.cfg.default_account == a.name;
                            let t = RichText::new(format!("claude-{}", a.name)).small();
                            let t = if on { t.color(acct_color(&a.name)).strong() } else { t.color(MUTED) };
                            if ui.selectable_label(on, t).clicked() {
                                acts.push(Act::SetDefault(a.name.clone()));
                            }
                        }
                        ui.label(RichText::new("Cuenta por defecto").small().color(MUTED));
                    });
                });
            });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(26.0)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(12, 3)).stroke(egui::Stroke::new(1.0_f32, LINE)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let branch = self.chat.as_ref().and_then(|id| self.rt.get(id)).map(|r| r.branch.clone()).unwrap_or_default();
                    if !branch.is_empty() {
                        ui.label(RichText::new(format!("rama {branch}")).small().color(MUTED));
                    }
                    let running = self.rt.values().filter(|r| r.st() == St::Running).count();
                    let agents: usize = self.rt.values().map(|r| r.tracker.running()).sum();
                    ui.label(RichText::new(format!("{running} chats activos · {agents} sub agentes")).small().color(MUTED));
                    let need = self.rt.values().filter(|r| r.attention()).count();
                    if need > 0 {
                        ui.label(RichText::new(format!("{need} te necesitan")).small().color(NEED));
                    }
                    if let Some((m, t)) = &self.toast {
                        if t.elapsed().as_secs() < 6 {
                            ui.add_space(12.0);
                            ui.label(RichText::new(m).small().color(OK));
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let acct = match self.bot.as_ref().and_then(|b| self.cfg.bot(b)) {
                            Some(b) => b.account.clone(),
                            None => self
                                .project
                                .as_ref()
                                .and_then(|p| self.cfg.project(p))
                                .map(|p| p.account.clone())
                                .unwrap_or(self.cfg.default_account.clone()),
                        };
                        ui.label(RichText::new(format!("claude-{acct}")).small().color(acct_color(&acct)));
                        dot(ui, acct_color(&acct));
                        let upd = self.upd.lock().map(|u| u.clone()).unwrap_or_default();
                        let gh = self.current_gh();
                        let label = match &gh {
                            Some(a) => {
                                let h = self.gh_hosts.iter().find(|h| &h.alias == a);
                                format!("GitHub: {}", h.map(|h| h.label()).unwrap_or(a.clone()))
                            }
                            None => "GitHub: elegir cuenta".into(),
                        };
                        if ui.selectable_label(self.gh_open, RichText::new(label).small()).clicked() {
                            self.gh_open = !self.gh_open;
                        }
                        match &upd {
                            Upd::Available(c) => {
                                let t = RichText::new(format!("Actualización disponible ({})", c.len())).small().strong().color(OK);
                                if ui.selectable_label(self.upd_open, t).clicked() {
                                    self.upd_open = true;
                                }
                            }
                            other => {
                                let tip = match other {
                                    Upd::NoRepo => "El código fuente no tiene un remoto git configurado".to_string(),
                                    Upd::UpToDate => "Al día con el repo".to_string(),
                                    Upd::Error(e) => format!("No se pudo revisar: {e}"),
                                    _ => "Revisando...".to_string(),
                                };
                                let v = format!("v{} · {}", env!("CARGO_PKG_VERSION"), updater::short_commit());
                                let r = ui.add(egui::Label::new(RichText::new(v).small().color(DIM)).sense(Sense::click()));
                                if r.on_hover_text(format!("{tip}\nClic para buscar actualizaciones")).clicked() {
                                    let st = self.upd.clone();
                                    let c2 = ctx.clone();
                                    std::thread::spawn(move || {
                                        let r = updater::check();
                                        if let Ok(mut s) = st.lock() {
                                            *s = r;
                                        }
                                        c2.request_repaint();
                                    });
                                    self.toast("Buscando actualizaciones...");
                                }
                            }
                        }
                    });
                });
            });
    }

    fn sidebar(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::SidePanel::left("side")
            .resizable(true)
            .default_width(280.0)
            .width_range(220.0..=420.0)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(10)).stroke(egui::Stroke::new(1.0_f32, LINE)))
            .show(ctx, |ui| {
                // Te necesita
                let mut inbox = Vec::new();
                for g in &self.cfg.groups {
                    for p in &g.projects {
                        for c in &p.chats {
                            if let Some(rt) = self.rt.get(&c.id) {
                                if rt.attention() {
                                    let why = if rt.st() == St::Needs { "pide permiso" } else { "terminó, revisar" };
                                    inbox.push((c.id.clone(), p.name.clone(), c.name.clone(), why, st_color(rt.st(), rt.review)));
                                }
                            }
                        }
                    }
                }
                if !inbox.is_empty() {
                    ui.label(RichText::new(format!("TE NECESITA  {}", inbox.len())).small().strong().color(NEED));
                    for (id, pn, cn, why, col) in inbox {
                        let t = RichText::new(format!("     {cn}\n     {pn} · {why}")).small().color(TEXT);
                        let b = egui::Button::new(t).fill(Color32::from_rgb(0x1A, 0x1C, 0x20)).min_size(vec2(ui.available_width(), 0.0));
                        let r = ui.add(b);
                        ui.painter().circle_filled(r.rect.left_top() + vec2(10.0, 11.0), 3.5, col);
                        if r.clicked() {
                            acts.push(Act::SelectChat(id));
                        }
                    }
                    ui.add_space(10.0);
                }

                ui.horizontal(|ui| {
                    ui.label(RichText::new("BOTS").small().strong().color(DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("+ bot").clicked() {
                            self.new_bot = Some(NewBot {
                                name: String::new(),
                                account: self.cfg.default_account.clone(),
                                projects: vec![],
                                worktree: true,
                                approve: false,
                            });
                        }
                    });
                });
                if self.cfg.bots.is_empty() {
                    ui.label(RichText::new("Un bot líder reparte tickets entre tus proyectos").small().color(DIM));
                }
                let bots_list = self.cfg.bots.clone();
                for b in &bots_list {
                    let sel = self.bot.as_deref() == Some(b.id.as_str());
                    let (st, review) = self.rt.get(&b.id).map(|r| (r.st(), r.review)).unwrap_or((St::Off, false));
                    let tasks = self.bot_tasks(&b.id);
                    let attention = tasks.iter().any(|t| t.3 == St::Needs || t.4);
                    ui.horizontal(|ui| {
                        dot(ui, st_color(st, review));
                        let r = ui.selectable_label(sel, RichText::new(&b.name).color(if sel { TEXT } else { Color32::from_gray(200) }));
                        if r.clicked() {
                            acts.push(Act::SelectBot(b.id.clone()));
                        }
                        r.context_menu(|ui| {
                            ui.label(RichText::new("Cuenta Claude del bot").small().color(MUTED));
                            for a in &self.cfg.accounts {
                                if ui.radio(b.account == a.name, format!("claude-{}", a.name)).clicked() {
                                    acts.push(Act::SetBotAccount(b.id.clone(), a.name.clone()));
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            let lbl = if b.allow_approve { "Quitar permiso de aprobar" } else { "Permitir que apruebe permisos" };
                            if ui.button(lbl).clicked() {
                                acts.push(Act::ToggleBotApprove(b.id.clone()));
                                ui.close_menu();
                            }
                            if ui.button("Reiniciar sesión").clicked() {
                                acts.push(Act::RestartBot(b.id.clone()));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Eliminar bot").clicked() {
                                acts.push(Act::DeleteBot(b.id.clone()));
                                ui.close_menu();
                            }
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let c = acct_color(&b.account);
                            let letter = b.account.chars().next().unwrap_or('?').to_ascii_uppercase().to_string();
                            ui.label(RichText::new(format!(" {letter} ")).small().strong().color(c).background_color(c.gamma_multiply(0.18)))
                                .on_hover_text(format!("claude-{}", b.account));
                            if !tasks.is_empty() {
                                ui.label(RichText::new(tasks.len().to_string()).small().color(DIM));
                            }
                            if attention {
                                dot(ui, NEED);
                            }
                        });
                    });
                    if sel {
                        ui.indent(("bot_tasks", &b.id), |ui| {
                            for (id, proj, title, st, review) in &tasks {
                                ui.horizontal(|ui| {
                                    dot(ui, st_color(*st, *review));
                                    let t = RichText::new(format!("{proj} · {title}")).size(12.0).color(MUTED);
                                    if ui.selectable_label(false, t).clicked() {
                                        acts.push(Act::SelectChat(id.clone()));
                                    }
                                });
                            }
                        });
                    }
                }
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("PROYECTOS").small().strong().color(DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("+ grupo").clicked() {
                            self.new_group = Some(String::new());
                        }
                        if ui.small_button("+ proyecto").clicked() {
                            acts.push(Act::AddProject(0));
                        }
                    });
                });
                let mut close_ng = false;
                if let Some(buf) = &mut self.new_group {
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(buf).hint_text("Nombre del grupo").desired_width(150.0));
                        r.request_focus();
                        if ui.small_button("Crear").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                            acts.push(Act::AddGroup(buf.clone()));
                            close_ng = true;
                        }
                        if ui.small_button("x").clicked() {
                            close_ng = true;
                        }
                    });
                }
                if close_ng {
                    self.new_group = None;
                }
                ui.add_space(4.0);

                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let ngroups = self.cfg.groups.len();
                    for (gi, g) in self.cfg.groups.iter().enumerate() {
                        let hr = ui.add(
                            egui::Label::new(RichText::new(format!("{}   {}", g.name, g.projects.len())).small().color(MUTED))
                                .sense(Sense::click()),
                        );
                        hr.context_menu(|ui| {
                            if ui.button("Agregar proyecto aquí").clicked() {
                                acts.push(Act::AddProject(gi));
                                ui.close_menu();
                            }
                            if g.projects.is_empty() && ui.button("Eliminar grupo").clicked() {
                                acts.push(Act::DeleteGroup(gi));
                                ui.close_menu();
                            }
                        });
                        let n = g.projects.len();
                        for (pi, p) in g.projects.iter().enumerate() {
                            let sel = self.project.as_deref() == Some(p.path.as_str());
                            let needy = p.chats.iter().any(|c| self.rt.get(&c.id).map(|r| r.attention()).unwrap_or(false));
                            ui.horizontal(|ui| {
                                let t = RichText::new(&p.name).color(if sel { TEXT } else { Color32::from_gray(200) });
                                let r = ui.selectable_label(sel, t);
                                if r.clicked() {
                                    acts.push(Act::SelectProject(p.path.clone()));
                                }
                                r.on_hover_text(&p.path).context_menu(|ui| {
                                    ui.label(RichText::new("Cuenta Claude").small().color(MUTED));
                                    for a in &self.cfg.accounts {
                                        if ui.radio(p.account == a.name, format!("claude-{}", a.name)).clicked() {
                                            acts.push(Act::SetAccount(p.path.clone(), a.name.clone()));
                                            ui.close_menu();
                                        }
                                    }
                                    ui.separator();
                                    ui.label(RichText::new("Mover a grupo").small().color(MUTED));
                                    for gj in 0..ngroups {
                                        if gj != gi && ui.button(&self.cfg.groups[gj].name).clicked() {
                                            acts.push(Act::MoveToGroup(p.path.clone(), gj));
                                            ui.close_menu();
                                        }
                                    }
                                    ui.separator();
                                    if ui.button("Quitar de tonycode").clicked() {
                                        acts.push(Act::RemoveProject(p.path.clone()));
                                        ui.close_menu();
                                    }
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if sel {
                                        if arrow_btn(ui, false, pi + 1 < n) {
                                            acts.push(Act::Move(gi, pi, 1));
                                        }
                                        if arrow_btn(ui, true, pi > 0) {
                                            acts.push(Act::Move(gi, pi, -1));
                                        }
                                    }
                                    let c = acct_color(&p.account);
                                    let letter = p.account.chars().next().unwrap_or('?').to_ascii_uppercase().to_string();
                                    ui.label(RichText::new(format!(" {letter} ")).small().strong().color(c).background_color(c.gamma_multiply(0.18)))
                                        .on_hover_text(format!("claude-{}", p.account));
                                    if !p.chats.is_empty() {
                                        ui.label(RichText::new(p.chats.len().to_string()).small().color(DIM));
                                    }
                                    if needy {
                                        dot(ui, NEED);
                                    }
                                });
                            });
                            if sel {
                                ui.indent(("chats", &p.path), |ui| {
                                    for c in &p.chats {
                                        let rt = self.rt.get(&c.id);
                                        let st = rt.map(|r| r.st()).unwrap_or(St::Off);
                                        let review = rt.map(|r| r.review).unwrap_or(false);
                                        let on = self.chat.as_deref() == Some(c.id.as_str());
                                        ui.horizontal(|ui| {
                                            dot(ui, st_color(st, review));
                                            let wt = if c.worktree.is_some() { "  · wt" } else { "" };
                                            let t = RichText::new(format!("{}{wt}", c.name)).size(12.5).color(if on { TEXT } else { MUTED });
                                            let r = ui.selectable_label(on, t);
                                            if r.clicked() {
                                                acts.push(Act::SelectChat(c.id.clone()));
                                            }
                                            r.context_menu(|ui| {
                                                if ui.button("Renombrar").clicked() {
                                                    self.rename = Some((c.id.clone(), c.name.clone()));
                                                    ui.close_menu();
                                                }
                                                if ui.button("Reiniciar sesión").clicked() {
                                                    acts.push(Act::Restart(c.id.clone()));
                                                    ui.close_menu();
                                                }
                                                if ui.button("Detener").clicked() {
                                                    acts.push(Act::Stop(c.id.clone()));
                                                    ui.close_menu();
                                                }
                                                ui.separator();
                                                if ui.button("Eliminar chat").clicked() {
                                                    acts.push(Act::DeleteChat(c.id.clone()));
                                                    ui.close_menu();
                                                }
                                            });
                                            if let Some(rt) = rt {
                                                if !rt.tracker.agents.is_empty() {
                                                    ui.label(RichText::new(format!("{} ag", rt.tracker.agents.len())).small().color(DIM));
                                                }
                                            }
                                        });
                                        if on {
                                            if let Some(rt) = rt {
                                                let ag = &rt.tracker.agents;
                                                for a in ag.iter().skip(ag.len().saturating_sub(6)) {
                                                    let col = if a.done { OK } else { RUN };
                                                    ui.label(
                                                        RichText::new(format!("  └ {} · {}", a.kind, a.desc)).monospace().size(11.0).color(col),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    if ui.add(egui::Button::new(RichText::new("+ Nuevo chat").small().color(MUTED)).frame(false)).clicked() {
                                        acts.push(Act::NewChat(p.path.clone()));
                                    }
                                });
                                ui.add_space(4.0);
                            }
                        }
                        ui.add_space(10.0);
                    }
                    if self.cfg.groups.iter().all(|g| g.projects.is_empty()) {
                        ui.label(RichText::new("Agrega tu primer proyecto con + proyecto").small().color(DIM));
                    }
                });
            });
    }

    fn central(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::CentralPanel::default().frame(Frame::new().fill(term::BG)).show(ctx, |ui| {
            if let Some(bid) = self.bot.clone() {
                self.bot_view(ui, ctx, &bid, acts);
                return;
            }
            let Some(cid) = self.chat.clone() else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Elige un proyecto o agrega uno con + proyecto\nCmd+K para buscar · Cmd+T nuevo chat").color(DIM));
                });
                return;
            };
            let Some((p, c)) = self.cfg.find_chat(&cid).map(|(p, c)| (p.clone(), c.clone())) else { return };
            let cwd = Self::chat_cwd(&p, &c);
            let (st, review) = self.rt.get(&cid).map(|r| (r.st(), r.review)).unwrap_or((St::Off, false));

            Frame::new().fill(HEAD).inner_margin(Margin::symmetric(12, 7)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    dot(ui, st_color(st, review));
                    ui.label(RichText::new(&c.name).strong().color(TEXT));
                    if let Some(b) = &c.branch {
                        ui.label(RichText::new(b).small().monospace().color(MUTED)).on_hover_text("Worktree propio");
                    }
                    if let Some(bn) = c.bot.as_ref().and_then(|b| self.cfg.bot(b)).map(|b| b.name.clone()) {
                        ui.label(RichText::new(format!("delegado por {bn}")).small().color(OTHER));
                    }
                    ui.label(RichText::new(format!("↺ {}", &c.session_id[..8])).small().color(DIM))
                        .on_hover_text(format!("Sesión {}\nSe retoma con claude --resume", c.session_id));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("+ Chat").on_hover_text("Cmd+T").clicked() {
                            acts.push(Act::NewChat(p.path.clone()));
                        }
                        ui.checkbox(&mut self.new_wt, RichText::new("worktree").small())
                            .on_hover_text("El chat nuevo trabaja en su propia rama y carpeta");
                        ui.separator();
                        for a in self.cfg.accounts.iter().rev() {
                            let on = p.account == a.name;
                            let t = RichText::new(&a.name).small();
                            let t = if on { t.color(acct_color(&a.name)).strong() } else { t.color(MUTED) };
                            if ui.selectable_label(on, t).on_hover_text("Cuenta con la que arranca este proyecto").clicked() {
                                acts.push(Act::SetAccount(p.path.clone(), a.name.clone()));
                            }
                        }
                        ui.separator();
                        if ui.selectable_label(self.diff_open, "Cambios").clicked() {
                            self.diff_open = !self.diff_open;
                            if self.diff_open {
                                self.files = false;
                                acts.push(Act::RefreshDiff);
                            }
                        }
                        if ui.selectable_label(self.files, "Archivos").clicked() {
                            self.files = !self.files;
                            if self.files {
                                self.diff_open = false;
                            }
                        }
                        if ui.button("↻").on_hover_text("Reiniciar sesión").clicked() {
                            acts.push(Act::Restart(cid.clone()));
                        }
                    });
                });
            });

            // Sub agentes
            if let Some(rt) = self.rt.get(&cid) {
                if !rt.tracker.agents.is_empty() {
                    Frame::new().inner_margin(Margin::symmetric(12, 6)).fill(term::BG).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("SUB AGENTES").small().strong().color(DIM));
                            let ag = &rt.tracker.agents;
                            for a in ag.iter().skip(ag.len().saturating_sub(8)) {
                                let col = if a.done { OK } else { RUN };
                                let txt = format!("{}  {}  {}", a.kind, a.desc, if a.done { "listo" } else { "corriendo" });
                                Frame::new()
                                    .fill(PANEL)
                                    .stroke(egui::Stroke::new(1.0_f32, LINE))
                                    .corner_radius(10)
                                    .inner_margin(Margin::symmetric(8, 2))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            dot(ui, col);
                                            ui.label(RichText::new(txt).small().color(TEXT));
                                        });
                                    });
                            }
                        });
                    });
                }
            }

            if self.files {
                egui::SidePanel::left("files_panel")
                    .resizable(true)
                    .default_width(640.0)
                    .frame(Frame::new().fill(Color32::from_rgb(0x14, 0x15, 0x18)).stroke(egui::Stroke::new(1.0_f32, LINE)))
                    .show_inside(ui, |ui| self.files_ui(ui, &cwd, acts));
            }
            if self.diff_open {
                egui::SidePanel::right("diff_panel")
                    .resizable(true)
                    .default_width(680.0)
                    .frame(Frame::new().fill(Color32::from_rgb(0x14, 0x15, 0x18)).inner_margin(Margin::same(10)).stroke(egui::Stroke::new(1.0_f32, LINE)))
                    .show_inside(ui, |ui| self.diff_ui(ui, acts));
            }

            self.term_view(ui, ctx, &cid, &cwd, false, acts);
        });
    }

    fn term_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, key: &str, cwd: &Path, is_bot: bool, acts: &mut Vec<Act>) {
        let want = std::mem::take(&mut self.want_focus);
        let focused = self.term_owns_keys(ctx);
        let input = std::mem::take(&mut self.term_input);
        let font = self.font.clone();
        let rt = self.rt.entry(key.to_string()).or_default();
        match &mut rt.term {
            Some(t) if t.is_alive() => {
                let r = t.ui(ui, &font, input, focused);
                if r.clicked() || want {
                    ctx.memory_mut(|m| {
                        if let Some(f) = m.focused() {
                            m.surrender_focus(f);
                        }
                    });
                }
            }
            _ => {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    let msg = match (&rt.err, rt.st()) {
                        (Some(e), _) => format!("No se pudo iniciar Claude:\n{e}"),
                        (None, St::Exited) => "La sesión de Claude terminó.".into(),
                        _ => "Sesión detenida.".into(),
                    };
                    ui.label(RichText::new(msg).color(MUTED));
                    ui.add_space(8.0);
                    ui.label(RichText::new(config::short(cwd)).small().monospace().color(DIM));
                    ui.add_space(12.0);
                    if ui.button("Iniciar sesión de Claude").clicked() {
                        acts.push(if is_bot { Act::RestartBot(key.to_string()) } else { Act::Restart(key.to_string()) });
                    }
                });
            }
        }
    }

    fn bot_tasks(&self, bid: &str) -> Vec<(String, String, String, St, bool)> {
        let mut v = Vec::new();
        for g in &self.cfg.groups {
            for p in &g.projects {
                for c in p.chats.iter().filter(|c| c.bot.as_deref() == Some(bid)) {
                    let (st, review) = self.rt.get(&c.id).map(|r| (r.st(), r.review)).unwrap_or((St::Off, false));
                    v.push((c.id.clone(), p.name.clone(), c.name.clone(), st, review));
                }
            }
        }
        v
    }

    fn bot_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, bid: &str, acts: &mut Vec<Act>) {
        let Some(bot) = self.cfg.bot(bid).cloned() else { return };
        let (st, review) = self.rt.get(bid).map(|r| (r.st(), r.review)).unwrap_or((St::Off, false));
        let tasks = self.bot_tasks(bid);
        Frame::new().fill(HEAD).inner_margin(Margin::symmetric(12, 7)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                dot(ui, st_color(st, review));
                ui.label(RichText::new(&bot.name).strong().color(TEXT));
                ui.label(RichText::new("lead bot").small().color(OTHER));
                let scope = if bot.projects.is_empty() { "todos los proyectos".to_string() } else { format!("{} proyectos", bot.projects.len()) };
                ui.label(RichText::new(format!("{} tareas · {scope}", tasks.len())).small().color(DIM));
                if bot.allow_approve {
                    ui.label(RichText::new("puede aprobar permisos").small().color(RUN));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("↻").on_hover_text("Reiniciar sesión del bot").clicked() {
                        acts.push(Act::RestartBot(bot.id.clone()));
                    }
                    ui.separator();
                    for a in self.cfg.accounts.iter().rev() {
                        let on = bot.account == a.name;
                        let t = RichText::new(&a.name).small();
                        let t = if on { t.color(acct_color(&a.name)).strong() } else { t.color(MUTED) };
                        if ui.selectable_label(on, t).on_hover_text("Cuenta de Claude con la que trabaja el bot").clicked() {
                            acts.push(Act::SetBotAccount(bot.id.clone(), a.name.clone()));
                        }
                    }
                });
            });
        });
        if !tasks.is_empty() {
            Frame::new().inner_margin(Margin::symmetric(12, 6)).fill(term::BG).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("TAREAS").small().strong().color(DIM));
                    for (id, proj, title, st, review) in &tasks {
                        Frame::new()
                            .fill(PANEL)
                            .stroke(egui::Stroke::new(1.0_f32, LINE))
                            .corner_radius(10)
                            .inner_margin(Margin::symmetric(8, 1))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    dot(ui, st_color(*st, *review));
                                    let b = egui::Button::new(RichText::new(format!("{proj} · {title}")).small().color(TEXT)).frame(false);
                                    if ui.add(b).on_hover_text(format!("Abrir tarea {}", &id[..8])).clicked() {
                                        acts.push(Act::SelectChat(id.clone()));
                                    }
                                });
                            });
                    }
                });
            });
        }
        let dir = bot.dir();
        self.term_view(ui, ctx, bid, &dir, true, acts);
    }

    fn files_ui(&mut self, ui: &mut egui::Ui, root: &Path, acts: &mut Vec<Act>) {
        ui.set_min_size(ui.available_size());
        let dir = self.browse.clone().filter(|d| d.starts_with(root)).unwrap_or_else(|| root.to_path_buf());
        let need = self.dir_cache.as_ref().map(|(d, _)| d != &dir).unwrap_or(true);
        if need {
            let mut entries: Vec<(String, bool)> = std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| (e.file_name().to_string_lossy().to_string(), e.file_type().map(|t| t.is_dir()).unwrap_or(false)))
                        .filter(|(n, _)| n != ".git" && n != ".DS_Store")
                        .collect()
                })
                .unwrap_or_default();
            entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
            self.dir_cache = Some((dir.clone(), entries));
        }
        egui::SidePanel::left("file_list")
            .resizable(true)
            .default_width(210.0)
            .frame(Frame::new().inner_margin(Margin::same(8)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Archivos").strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("x").clicked() {
                            self.files = false;
                        }
                        if ui.small_button("↻").clicked() {
                            self.dir_cache = None;
                        }
                    });
                });
                let rel = dir.strip_prefix(root).map(|r| r.display().to_string()).unwrap_or_default();
                ui.label(RichText::new(format!("./{rel}")).small().monospace().color(DIM));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if dir != root {
                        if ui.selectable_label(false, RichText::new("..").color(MUTED)).clicked() {
                            self.browse = dir.parent().map(|p| p.to_path_buf());
                        }
                    }
                    if let Some((_, entries)) = &self.dir_cache {
                        for (name, is_dir) in entries {
                            let path = dir.join(name);
                            let open = self.editor.as_ref().map(|e| e.path == path).unwrap_or(false);
                            let t = if *is_dir {
                                RichText::new(format!("{name}/")).color(MUTED)
                            } else {
                                RichText::new(name).color(Color32::from_gray(200))
                            };
                            if ui.selectable_label(open, t).clicked() {
                                if *is_dir {
                                    self.browse = Some(path);
                                } else {
                                    acts.push(Act::OpenFile(path));
                                }
                            }
                        }
                    }
                });
            });
        egui::CentralPanel::default().frame(Frame::new().inner_margin(Margin::same(8))).show_inside(ui, |ui| {
            let mut save = false;
            let mut discard = false;
            if let Some(ed) = &mut self.editor {
                ui.horizontal(|ui| {
                    let rel = ed.path.strip_prefix(root).map(|r| r.display().to_string()).unwrap_or(ed.path.display().to_string());
                    ui.label(RichText::new(rel).small().monospace());
                    if ed.dirty {
                        ui.label(RichText::new("● sin guardar").small().color(RUN));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(ed.dirty, egui::Button::new("Guardar")).on_hover_text("Cmd+S").clicked() {
                            save = true;
                        }
                        if ed.dirty && ui.small_button("Descartar").clicked() {
                            discard = true;
                        }
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut ed.text)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(40)
                            .frame(false),
                    );
                    if r.changed() {
                        ed.dirty = true;
                    }
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Elige un archivo para editarlo").color(DIM));
                });
            }
            if save {
                self.save_editor();
            }
            if discard {
                self.editor = None;
            }
        });
    }

    fn diff_ui(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        ui.set_min_size(ui.available_size());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Cambios").strong());
            let n = self.diff.as_ref().map(|d| d.status.len()).unwrap_or(0);
            ui.label(RichText::new(format!("{n} archivos")).small().color(MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("x").clicked() {
                    self.diff_open = false;
                }
                if ui.small_button("↻").clicked() {
                    acts.push(Act::RefreshDiff);
                }
            });
        });
        ui.add_space(6.0);
        let has = self.diff.as_ref().map(|d| !d.status.is_empty()).unwrap_or(false);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.commit_msg).hint_text("Mensaje del commit").desired_width(ui.available_width() - 170.0));
            if ui.add_enabled(has && !self.commit_msg.trim().is_empty(), egui::Button::new("Commit")).clicked() {
                acts.push(Act::Commit);
            }
            if ui.add_enabled(has, egui::Button::new("Revertir")).clicked() {
                self.confirm_revert = true;
            }
        });
        ui.add_space(6.0);
        if let Some(d) = &self.diff {
            for s in &d.status {
                ui.label(RichText::new(s).small().monospace().color(MUTED));
            }
            ui.separator();
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                for (c, l) in &d.lines {
                    ui.label(RichText::new(l).monospace().size(12.0).color(*c));
                }
                if d.status.is_empty() {
                    ui.label(RichText::new("Sin cambios pendientes.").color(DIM));
                }
            });
        }
    }

    fn modals(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let mut nb_create = false;
        let mut nb_cancel = false;
        if let Some(nb) = &mut self.new_bot {
            let m = egui::Modal::new(egui::Id::new("new_bot")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new("Nuevo bot líder").strong().size(16.0));
                ui.add_space(6.0);
                ui.label(RichText::new("Nombre").small().color(MUTED));
                ui.add(egui::TextEdit::singleline(&mut nb.name).hint_text("Aceable Bot").desired_width(f32::INFINITY));
                ui.add_space(6.0);
                ui.label(RichText::new("Cuenta de Claude con la que trabaja").small().color(MUTED));
                ui.horizontal(|ui| {
                    for a in &self.cfg.accounts {
                        let t = RichText::new(format!("claude-{}", a.name)).color(acct_color(&a.name));
                        if ui.radio(nb.account == a.name, t).clicked() {
                            nb.account = a.name.clone();
                        }
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Proyectos a los que puede delegar (ninguno marcado = todos)").small().color(MUTED));
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for g in &self.cfg.groups {
                        if g.projects.is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(&g.name).small().color(DIM));
                        for p in &g.projects {
                            let mut on = nb.projects.contains(&p.path);
                            if ui.checkbox(&mut on, format!("{}   (claude-{})", p.name, p.account)).changed() {
                                if on {
                                    nb.projects.push(p.path.clone());
                                } else {
                                    nb.projects.retain(|x| x != &p.path);
                                }
                            }
                        }
                    }
                });
                ui.add_space(6.0);
                ui.checkbox(&mut nb.worktree, "Cada tarea en su propio worktree (rama aparte)");
                ui.checkbox(&mut nb.approve, "Puede responder prompts de permiso");
                if nb.approve {
                    ui.label(RichText::new("El bot podrá aceptar comandos en las tareas sin pasar por ti. Sus instrucciones le prohíben aprobar push, merge, deploy o borrados, pero es tu decisión.").small().color(RUN));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Crear bot").strong()).clicked() {
                        nb_create = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        nb_cancel = true;
                    }
                });
            });
            if m.should_close() {
                nb_cancel = true;
            }
        }
        if nb_create {
            acts.push(Act::CreateBot);
        } else if nb_cancel {
            self.new_bot = None;
        }

        if self.pal {
            let q = self.pal_q.to_lowercase();
            let mut res: Vec<(String, String, Act)> = Vec::new();
            for g in &self.cfg.groups {
                for p in &g.projects {
                    if p.name.to_lowercase().contains(&q) {
                        res.push((format!("PROYECTO   {}", p.name), g.name.clone(), Act::SelectProject(p.path.clone())));
                    }
                    for c in &p.chats {
                        if !q.is_empty() && c.name.to_lowercase().contains(&q) {
                            res.push((format!("CHAT          {}", c.name), p.name.clone(), Act::SelectChat(c.id.clone())));
                        }
                    }
                }
            }
            res.truncate(12);
            let mut pick: Option<usize> = None;
            let m = egui::Modal::new(egui::Id::new("palette")).show(ctx, |ui| {
                ui.set_width(520.0);
                let r = ui.add(egui::TextEdit::singleline(&mut self.pal_q).hint_text("Proyecto o chat...").desired_width(f32::INFINITY));
                if self.pal_focus {
                    r.request_focus();
                    self.pal_focus = false;
                }
                if ui.input(|i| i.key_pressed(Key::Enter)) && !res.is_empty() {
                    pick = Some(0);
                }
                ui.add_space(6.0);
                for (i, (label, sub, _)) in res.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(i == 0, RichText::new(label)).clicked() {
                            pick = Some(i);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(sub).small().color(DIM));
                        });
                    });
                }
                if res.is_empty() {
                    ui.label(RichText::new("Sin resultados").color(DIM));
                }
            });
            if let Some(i) = pick {
                acts.push(res.swap_remove(i).2);
                self.pal = false;
            } else if m.should_close() {
                self.pal = false;
            }
        }

        let mut close_rename = false;
        if let Some((id, buf)) = &mut self.rename {
            let m = egui::Modal::new(egui::Id::new("rename")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label("Nombre del chat");
                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY));
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button("Guardar").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        acts.push(Act::Rename(id.clone(), buf.trim().to_string()));
                        close_rename = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        close_rename = true;
                    }
                });
            });
            if m.should_close() {
                close_rename = true;
            }
        }
        if close_rename {
            self.rename = None;
            self.want_focus = true;
        }

        if self.confirm_revert {
            let m = egui::Modal::new(egui::Id::new("revert")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new("¿Revertir todos los cambios de este chat?").strong());
                ui.label(RichText::new("Se descartan los cambios en archivos con seguimiento. No se puede deshacer.").small().color(MUTED));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Revertir").color(NEED)).clicked() {
                        acts.push(Act::Revert);
                        self.confirm_revert = false;
                    }
                    if ui.button("Cancelar").clicked() {
                        self.confirm_revert = false;
                    }
                });
            });
            if m.should_close() {
                self.confirm_revert = false;
            }
        }

        if self.upd_open {
            let mut open = true;
            let upd = self.upd.lock().map(|u| u.clone()).unwrap_or_default();
            let mut start = false;
            let mut restart = false;
            egui::Window::new("Actualizar tonycode")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.set_width(780.0);
                    if let Upd::Available(commits) = &upd {
                        ui.label(RichText::new(format!("{} commits nuevos en el repo:", commits.len())).color(MUTED));
                        for c in commits.iter().take(12) {
                            ui.label(RichText::new(c).small().monospace());
                        }
                    }
                    ui.add_space(8.0);
                    match &mut self.upd_term {
                        None => {
                            ui.label(RichText::new("Se hará git pull, se compilará y se reinstalará la app. Tus chats se retoman solos con --resume al volver a abrir.").small().color(DIM));
                            ui.add_space(6.0);
                            if ui.button(RichText::new("Actualizar ahora").strong()).clicked() {
                                start = true;
                            }
                        }
                        Some(t) => {
                            let alive = t.is_alive();
                            let ok = t.screen_text().contains("TONYCODE_UPDATE_OK");
                            ui.allocate_ui(vec2(780.0, 340.0), |ui| {
                                ui.set_min_size(vec2(780.0, 340.0));
                                t.ui(ui, &FontId::monospace(12.0), vec![], false);
                            });
                            ui.add_space(6.0);
                            if alive {
                                ui.label(RichText::new("Actualizando... (la compilación tarda un par de minutos)").small().color(RUN));
                            } else if ok {
                                ui.label(RichText::new("Listo. Reinicia para usar la nueva versión.").color(OK));
                                if ui.button(RichText::new("Reiniciar tonycode").strong()).clicked() {
                                    restart = true;
                                }
                            } else {
                                ui.label(RichText::new("La actualización falló. Revisa la salida de arriba.").color(NEED));
                            }
                        }
                    }
                });
            if start {
                let (prog, args) = updater::update_command();
                match Term::spawn(&prog, &args, &updater::source_dir(), &[], ctx.clone()) {
                    Ok(t) => self.upd_term = Some(t),
                    Err(e) => self.toast(format!("No se pudo iniciar la actualización: {e}")),
                }
            }
            if restart {
                self.cfg.save();
                self.rt.clear();
                updater::relaunch();
            }
            if !open {
                self.upd_open = false;
                if self.upd_term.as_ref().map(|t| !t.is_alive()).unwrap_or(false) {
                    self.upd_term = None;
                }
            }
        }

        if self.gh_open {
            let mut open = true;
            let cur = self.current_gh();
            egui::Window::new("Cuenta de GitHub activa")
                .anchor(Align2::RIGHT_BOTTOM, [-12.0, -34.0])
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.set_width(380.0);
                    if self.gh_hosts.is_empty() {
                        ui.label(RichText::new("No encontré hosts de GitHub en ~/.ssh/config").color(MUTED));
                    }
                    for h in &self.gh_hosts {
                        let on = cur.as_deref() == Some(h.alias.as_str());
                        ui.horizontal(|ui| {
                            if ui.radio(on, RichText::new(h.label()).strong()).clicked() {
                                acts.push(Act::SetGh(h.alias.clone()));
                            }
                            ui.label(RichText::new(&h.alias).small().monospace().color(MUTED));
                        });
                        ui.label(RichText::new(format!("      {}", h.key)).small().monospace().color(DIM));
                    }
                    ui.separator();
                    let proj = self.project.as_ref().and_then(|p| self.cfg.project(p)).map(|p| p.name.clone());
                    ui.label(
                        RichText::new(match proj {
                            Some(n) => format!("Al elegir, se aplica git config core.sshCommand en {n}"),
                            None => "Elige un proyecto para aplicarla en su repo".into(),
                        })
                        .small()
                        .color(MUTED),
                    );
                    if ui.small_button("↻ Releer ~/.ssh/config").clicked() {
                        self.gh_hosts = ssh::github_hosts();
                    }
                });
            if !open {
                self.gh_open = false;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut acts = Vec::new();
        self.shortcuts(ctx, &mut acts);
        self.grab_term_input(ctx);
        self.scan(ctx);
        self.process_inbox(ctx);
        self.write_bot_status();
        self.top_bar(ctx, &mut acts);
        self.status_bar(ctx);
        self.sidebar(ctx, &mut acts);
        self.central(ctx, &mut acts);
        self.modals(ctx, &mut acts);
        for a in acts {
            self.apply(a, ctx);
        }
        if self.rt.values().any(|r| r.term.is_some()) {
            ctx.request_repaint_after(Duration::from_millis(700));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cfg.save();
        self.rt.clear();
    }
}

fn notify(msg: &str) {
    #[cfg(target_os = "macos")]
    {
        let m = msg.replace('"', "'");
        let _ = std::process::Command::new("osascript")
            .args(["-e", &format!("display notification \"{m}\" with title \"tonycode\"")])
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = msg;
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates: [(&str, &str, bool); 3] = [
        ("menlo", "/System/Library/Fonts/Menlo.ttc", true),
        ("symbols", "/System/Library/Fonts/Apple Symbols.ttf", false),
        ("sfns", "/System/Library/Fonts/SFNS.ttf", false),
    ];
    for (name, path, mono_first) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            if mono_first {
                fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, name.into());
            } else if name == "sfns" {
                fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, name.into());
            } else {
                for fam in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
                    fonts.families.entry(fam).or_default().push(name.into());
                }
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = HEAD;
    v.window_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.selection.bg_fill = SEL;
    v.selection.stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x1E, 0x20, 0x24);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x26, 0x29, 0x2F);
    v.widgets.active.weak_bg_fill = SEL;
    v.override_text_color = None;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(6.0, 4.0);
        s.spacing.button_padding = vec2(8.0, 3.0);
    });
}

fn chrono_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let d = secs % 86400;
    format!("{:02}:{:02}:{:02} UTC", d / 3600, (d / 60) % 60, d % 60)
}
