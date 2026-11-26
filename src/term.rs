//! Terminal embebida: PTY real + parser vt100, dibujada con egui.

use crate::i18n::tr;
use crate::trf;
use egui::{Color32, Event, FontId, Key, Modifiers, Rect, Sense, pos2, vec2};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Tema de la terminal: fondo, texto, cursor, selección y los 16 colores ANSI.
pub struct Theme {
    pub name: &'static str,
    pub bg: u32,
    pub fg: u32,
    pub cursor: u32,
    pub sel: u32,
    pub ansi: [u32; 16],
}

pub const THEMES: &[Theme] = &[
    Theme { name: "tonycode", bg: 0x0D0E10, fg: 0xD7DAE0, cursor: 0xF0A070, sel: 0x8AB4FF,
        ansi: [0x1E2024, 0xE06C75, 0x98C379, 0xE5C07B, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xABB2BF, 0x5C6370, 0xF07178, 0xB5E08A, 0xF0C060, 0x8AB4FF, 0xD7A6F0, 0x7FD8D0, 0xE6E6E6] },
    Theme { name: "Dracula", bg: 0x282A36, fg: 0xF8F8F2, cursor: 0xF8F8F2, sel: 0xBD93F9,
        ansi: [0x21222C, 0xFF5555, 0x50FA7B, 0xF1FA8C, 0xBD93F9, 0xFF79C6, 0x8BE9FD, 0xF8F8F2, 0x6272A4, 0xFF6E6E, 0x69FF94, 0xFFFFA5, 0xD6ACFF, 0xFF92DF, 0xA4FFFF, 0xFFFFFF] },
    Theme { name: "Catppuccin Mocha", bg: 0x1E1E2E, fg: 0xCDD6F4, cursor: 0xF5E0DC, sel: 0x89B4FA,
        ansi: [0x45475A, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xBAC2DE, 0x585B70, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xA6ADC8] },
    Theme { name: "Tokyo Night", bg: 0x1A1B26, fg: 0xC0CAF5, cursor: 0xC0CAF5, sel: 0x7AA2F7,
        ansi: [0x15161E, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xA9B1D6, 0x414868, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xC0CAF5] },
    Theme { name: "One Dark", bg: 0x282C34, fg: 0xABB2BF, cursor: 0x528BFF, sel: 0x61AFEF,
        ansi: [0x282C34, 0xE06C75, 0x98C379, 0xE5C07B, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xABB2BF, 0x5C6370, 0xE06C75, 0x98C379, 0xE5C07B, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xFFFFFF] },
    Theme { name: "Nord", bg: 0x2E3440, fg: 0xD8DEE9, cursor: 0xD8DEE9, sel: 0x88C0D0,
        ansi: [0x3B4252, 0xBF616A, 0xA3BE8C, 0xEBCB8B, 0x81A1C1, 0xB48EAD, 0x88C0D0, 0xE5E9F0, 0x4C566A, 0xBF616A, 0xA3BE8C, 0xEBCB8B, 0x81A1C1, 0xB48EAD, 0x8FBCBB, 0xECEFF4] },
    Theme { name: "Gruvbox Dark", bg: 0x282828, fg: 0xEBDBB2, cursor: 0xEBDBB2, sel: 0x83A598,
        ansi: [0x282828, 0xCC241D, 0x98971A, 0xD79921, 0x458588, 0xB16286, 0x689D6A, 0xA89984, 0x928374, 0xFB4934, 0xB8BB26, 0xFABD2F, 0x83A598, 0xD3869B, 0x8EC07C, 0xEBDBB2] },
    Theme { name: "Solarized Dark", bg: 0x002B36, fg: 0x93A1A1, cursor: 0x93A1A1, sel: 0x268BD2,
        ansi: [0x073642, 0xDC322F, 0x859900, 0xB58900, 0x268BD2, 0xD33682, 0x2AA198, 0xEEE8D5, 0x586E75, 0xCB4B16, 0x586E75, 0x657B83, 0x839496, 0x6C71C4, 0x93A1A1, 0xFDF6E3] },
];

static THEME_IDX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn set_theme(name: &str) {
    let i = THEMES.iter().position(|t| t.name == name).unwrap_or(0);
    THEME_IDX.store(i, Ordering::Relaxed);
}

pub fn theme() -> &'static Theme {
    &THEMES[THEME_IDX.load(Ordering::Relaxed).min(THEMES.len() - 1)]
}

pub fn bg() -> Color32 {
    hex(theme().bg)
}

fn fg() -> Color32 {
    hex(theme().fg)
}

#[derive(Clone, Copy, PartialEq)]
pub enum CursorKind {
    Bar,
    Block,
    Underline,
}

/// Estilo visual de la terminal (viene de la configuración).
#[derive(Clone)]
pub struct Style {
    pub size: f32,
    pub line: f32,
    pub cursor: CursorKind,
}

fn family(bold: bool, italic: bool) -> egui::FontFamily {
    match (bold, italic) {
        (false, false) => egui::FontFamily::Monospace,
        (true, false) => egui::FontFamily::Name("tc-bold".into()),
        (false, true) => egui::FontFamily::Name("tc-italic".into()),
        (true, true) => egui::FontFamily::Name("tc-bolditalic".into()),
    }
}

pub struct Cb {
    pub bell: bool,
}

impl vt100::Callbacks for Cb {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
    }
}

fn nz(v: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(v.max(1)).unwrap()
}

pub struct Term {
    pub parser: Arc<Mutex<vt100::Parser<Cb>>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    pub alive: Arc<AtomicBool>,
    pub last_output: Arc<Mutex<Instant>>,
    pub started: Instant,
    size: (u16, u16),
    scroll_acc: f32,
    sel: Option<((u16, u16), (u16, u16))>,
    copied_at: Option<Instant>,
    /// Texto en composición (tecla muerta, IME): se muestra en el cursor hasta confirmarse.
    preedit: String,
    /// Posición del cursor en pantalla (para ubicar la ventana del IME).
    pub last_cursor: Option<Rect>,
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

impl Term {
    pub fn spawn(
        program: &str,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
        ctx: egui::Context,
    ) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            nz(30),
            nz(100),
            5000,
            Cb { bell: false },
        )));
        let alive = Arc::new(AtomicBool::new(true));
        let last = Arc::new(Mutex::new(Instant::now()));
        {
            let parser = parser.clone();
            let alive = alive.clone();
            let last = last.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 16384];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut p) = parser.lock() {
                                p.process(&buf[..n]);
                            }
                            if let Ok(mut l) = last.lock() {
                                *l = Instant::now();
                            }
                            ctx.request_repaint();
                        }
                    }
                }
                alive.store(false, Ordering::SeqCst);
                ctx.request_repaint();
            });
        }
        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            alive,
            last_output: last,
            started: Instant::now(),
            size: (30, 100),
            scroll_acc: 0.0,
            sel: None,
            copied_at: None,
            preedit: String::new(),
            last_cursor: None,
        })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn idle_secs(&self) -> f32 {
        self.last_output.lock().map(|l| l.elapsed().as_secs_f32()).unwrap_or(99.0)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Pega texto respetando el modo "bracketed paste" del programa.
    pub fn paste(&mut self, t: &str) {
        let bracketed = self.parser.lock().map(|p| p.screen().bracketed_paste()).unwrap_or(false);
        let mut out = Vec::new();
        if bracketed {
            out.extend_from_slice(b"\x1b[200~");
            out.extend_from_slice(t.as_bytes());
            out.extend_from_slice(b"\x1b[201~");
        } else {
            out.extend_from_slice(t.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
        }
        self.write(&out);
    }

    #[allow(dead_code)]
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(nz(rows), nz(cols));
        }
    }

    /// Texto visible de la pantalla (para detectar prompts de permiso).
    pub fn screen_text(&self) -> String {
        self.parser.lock().map(|p| p.screen().contents()).unwrap_or_default()
    }

    pub fn take_bell(&self) -> bool {
        if let Ok(mut p) = self.parser.lock() {
            let b = p.callbacks().bell;
            p.callbacks_mut().bell = false;
            b
        } else {
            false
        }
    }

    fn scroll_lines(&mut self, lines: i64) {
        if let Ok(mut p) = self.parser.lock() {
            let cur = p.screen().scrollback() as i64;
            p.screen_mut().set_scrollback((cur + lines).max(0) as usize);
        }
        self.sel = None;
    }

    /// Rueda del mouse: historial propio, o se reenvía al programa si usa mouse / pantalla alterna.
    fn wheel(&mut self, lines: i64, cell: (u16, u16)) {
        let (mode, enc, alt, app_cursor) = match self.parser.lock() {
            Ok(p) => {
                let s = p.screen();
                (s.mouse_protocol_mode(), s.mouse_protocol_encoding(), s.alternate_screen(), s.application_cursor())
            }
            Err(_) => return,
        };
        let n = lines.unsigned_abs().min(10) as usize;
        if mode != vt100::MouseProtocolMode::None {
            let btn = if lines > 0 { 64 } else { 65 };
            let (r, c) = (cell.0 as u32 + 1, cell.1 as u32 + 1);
            let seq = if enc == vt100::MouseProtocolEncoding::Sgr {
                format!("\x1b[<{btn};{c};{r}M")
            } else {
                let ch = |v: u32| char::from_u32((v + 32).min(255)).unwrap_or(' ');
                format!("\x1b[M{}{}{}", ch(btn), ch(c), ch(r))
            };
            let out = seq.repeat(n);
            self.write(out.as_bytes());
        } else if alt {
            let k = match (lines > 0, app_cursor) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            self.write(k.repeat(n).as_bytes());
        } else {
            self.scroll_lines(lines);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, style: &Style, input: Vec<Event>, focused: bool) -> egui::Response {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let font = FontId::new(style.size, egui::FontFamily::Monospace);
        // Ancho real de celda: medido sobre una línea larga para que coincida con cómo egui
        // coloca los caracteres (evita que el texto se corra respecto al cursor).
        let (cw, text_h) = ui.fonts(|f| {
            let g = f.layout_no_wrap("M".repeat(200), font.clone(), Color32::WHITE);
            (g.size().x / 200.0, f.row_height(&font))
        });
        let rh = (text_h * style.line).round();
        let y_off = ((rh - text_h) / 2.0).floor();
        let pad = 12.0;
        let cols = (((rect.width() - pad * 2.0) / cw).floor() as u16).max(20);
        let rows = (((rect.height() - pad * 2.0) / rh).floor() as u16).max(5);
        self.resize(rows, cols);
        let origin = rect.min + vec2(pad, pad);
        let to_cell = |p: egui::Pos2| -> (u16, u16) {
            let c = ((p.x - origin.x) / cw).floor().clamp(0.0, (cols - 1) as f32) as u16;
            let r = ((p.y - origin.y) / rh).floor().clamp(0.0, (rows - 1) as f32) as u16;
            (r, c)
        };
        if !input.is_empty() {
            self.handle_input(input);
        }

        // Rueda / trackpad
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                self.scroll_acc += dy / rh;
                let lines = self.scroll_acc.trunc() as i64;
                if lines != 0 {
                    self.scroll_acc -= lines as f32;
                    let cell = ui.input(|i| i.pointer.hover_pos()).map(to_cell).unwrap_or((0, 0));
                    self.wheel(lines, cell);
                }
            }
        }

        // Selección: arrastrar, doble clic (palabra), triple clic (línea). Se copia sola.
        let mut copy_now = false;
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                let c = to_cell(p);
                self.sel = Some((c, c));
            }
        } else if resp.dragged_by(egui::PointerButton::Primary) {
            if let (Some(p), Some((a, _))) = (ui.input(|i| i.pointer.interact_pos()), self.sel) {
                self.sel = Some((a, to_cell(p)));
            }
        }
        if resp.drag_stopped() && self.sel.map(|(a, b)| a != b).unwrap_or(false) {
            copy_now = true;
        }
        if resp.triple_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let (r, _) = to_cell(p);
                self.sel = Some(((r, 0), (r, cols - 1)));
                copy_now = true;
            }
        } else if resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let (r, c) = to_cell(p);
                if let Ok(par) = self.parser.lock() {
                    let s = par.screen();
                    let is_word = |c: u16| {
                        s.cell(r, c)
                            .map(|x| {
                                let t = x.contents();
                                !t.is_empty() && !t.chars().all(|ch| ch.is_whitespace() || "│|\"'()[]{}<>,;`".contains(ch))
                            })
                            .unwrap_or(false)
                    };
                    if is_word(c) {
                        let mut a = c;
                        while a > 0 && is_word(a - 1) {
                            a -= 1;
                        }
                        let mut b = c;
                        while b + 1 < cols && is_word(b + 1) {
                            b += 1;
                        }
                        self.sel = Some(((r, a), (r, b)));
                        copy_now = true;
                    }
                }
            }
        } else if resp.clicked() {
            self.sel = None;
        }
        if copy_now {
            if let Some(t) = self.selected_text(cols) {
                if !t.trim().is_empty() {
                    ui.ctx().copy_text(t);
                    self.copied_at = Some(Instant::now());
                }
            }
        }

        let painter = ui.painter_at(rect);
        let bgc = bg();
        painter.rect_filled(rect, 0.0, bgc);
        let sel = self.sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
        let Ok(p) = self.parser.lock() else { return resp };
        let s = p.screen();
        let stroke_w = (rh / 13.0).max(1.0);
        for r in 0..rows {
            let y = origin.y + r as f32 * rh;
            let mut c = 0u16;
            while c < cols {
                let Some(cell) = s.cell(r, c) else { break };
                if cell.is_wide_continuation() {
                    c += 1;
                    continue;
                }
                let (fg, bg) = colors(cell);
                let start = c;
                let mut text = String::new();
                let t = cell.contents();
                let mut special: Option<char> = None;
                if t.is_empty() || t.is_ascii() {
                    text.push_str(if t.is_empty() { " " } else { t });
                    c += 1;
                    while c < cols {
                        let Some(n) = s.cell(r, c) else { break };
                        let nt = n.contents();
                        if n.is_wide_continuation()
                            || !(nt.is_empty() || nt.is_ascii())
                            || colors(n) != (fg, bg)
                            || n.bold() != cell.bold()
                            || n.italic() != cell.italic()
                            || n.underline() != cell.underline()
                        {
                            break;
                        }
                        text.push_str(if nt.is_empty() { " " } else { nt });
                        c += 1;
                    }
                } else {
                    text.push_str(t);
                    let mut it = t.chars();
                    if let (Some(ch), None) = (it.next(), it.next()) {
                        if is_drawn(ch) {
                            special = Some(ch);
                        }
                    }
                    c += if cell.is_wide() { 2 } else { 1 };
                }
                let x = origin.x + start as f32 * cw;
                let w = (c - start) as f32 * cw;
                let cell_rect = Rect::from_min_size(pos2(x, y), vec2(w, rh));
                if bg != bgc {
                    painter.rect_filled(cell_rect, 0.0, bg);
                }
                if let Some(ch) = special {
                    draw_special(&painter, cell_rect, ch, fg, stroke_w);
                } else if !text.trim().is_empty() {
                    let f = FontId::new(style.size, family(cell.bold(), cell.italic()));
                    painter.text(pos2(x, y + y_off), egui::Align2::LEFT_TOP, text, f, fg);
                }
                if cell.underline() {
                    let uy = y + y_off + text_h - 1.0;
                    painter.line_segment([pos2(x, uy), pos2(x + w, uy)], egui::Stroke::new(1.0_f32, fg));
                }
            }
            // resaltado de selección
            if let Some(((sr, sc), (er, ec))) = sel {
                if r >= sr && r <= er {
                    let c0 = if r == sr { sc } else { 0 };
                    let c1 = if r == er { ec } else { cols - 1 };
                    let rr = Rect::from_min_max(pos2(origin.x + c0 as f32 * cw, y), pos2(origin.x + (c1 + 1) as f32 * cw, y + rh));
                    let sc = hex(theme().sel);
                    painter.rect_filled(rr, 0.0, Color32::from_rgba_unmultiplied(sc.r(), sc.g(), sc.b(), 80));
                }
            }
        }
        if !s.hide_cursor() && s.scrollback() == 0 {
            let (cr, cc) = s.cursor_position();
            let r = Rect::from_min_size(origin + vec2(cc as f32 * cw, cr as f32 * rh), vec2(cw, rh));
            self.last_cursor = Some(r);
            let cc = hex(theme().cursor);
            if !self.preedit.is_empty() {
                let f = FontId::new(style.size, egui::FontFamily::Monospace);
                let g = painter.layout_no_wrap(self.preedit.clone(), f, cc);
                let pr = Rect::from_min_size(r.min, vec2(g.size().x.max(cw), rh));
                painter.rect_filled(pr, 0.0, bgc);
                painter.galley(pos2(r.min.x, r.min.y + y_off), g, cc);
                painter.line_segment([pos2(pr.min.x, r.min.y + y_off + text_h), pos2(pr.max.x, r.min.y + y_off + text_h)], egui::Stroke::new(1.0_f32, cc));
            }
            if focused {
                match style.cursor {
                    CursorKind::Bar => {
                        painter.rect_filled(Rect::from_min_size(r.min + vec2(0.0, y_off), vec2(2.0, text_h)), 0.0, cc);
                    }
                    CursorKind::Underline => {
                        painter.rect_filled(Rect::from_min_size(pos2(r.min.x, r.min.y + y_off + text_h - 2.0), vec2(cw, 2.0)), 0.0, cc);
                    }
                    CursorKind::Block => {
                        painter.rect_filled(r, 1.0, Color32::from_rgba_unmultiplied(cc.r(), cc.g(), cc.b(), 170));
                    }
                }
            } else {
                painter.rect_stroke(r, 1.0, egui::Stroke::new(1.0_f32, cc.gamma_multiply(0.5)), egui::StrokeKind::Inside);
            }
        }
        let mut badge: Option<String> = None;
        if !ui.input(|i| i.raw.hovered_files.is_empty()) && resp.contains_pointer() {
            painter.rect_filled(rect.shrink(6.0), 10.0, Color32::from_rgba_unmultiplied(138, 180, 255, 28));
            painter.rect_stroke(rect.shrink(6.0), 10.0, egui::Stroke::new(1.5_f32, hex(theme().sel)), egui::StrokeKind::Inside);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, tr("Suelta para adjuntar a Claude", "Drop to attach to Claude"), FontId::proportional(15.0), Color32::from_gray(230));
        }
        if s.scrollback() > 0 {
            badge = Some(trf!("↑ {} líneas · Shift+PgDn o escribe para volver", "↑ {} lines · Shift+PgDn or type to return", s.scrollback()));
        }
        if self.copied_at.map(|t| t.elapsed().as_secs_f32() < 1.4).unwrap_or(false) {
            badge = Some(tr("Copiado al portapapeles", "Copied to clipboard").into());
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        if let Some(b) = badge {
            let pos = rect.right_top() + vec2(-12.0, 8.0);
            let g = painter.layout_no_wrap(b, FontId::proportional(11.5), Color32::from_gray(220));
            let br = Rect::from_min_size(pos - vec2(g.size().x, 0.0), g.size()).expand2(vec2(8.0, 4.0));
            painter.rect_filled(br, 6.0, Color32::from_rgb(0x23, 0x26, 0x2C));
            painter.galley(br.min + vec2(8.0, 4.0), g, Color32::from_gray(220));
        }
        resp
    }

    fn selected_text(&self, cols: u16) -> Option<String> {
        let (a, b) = self.sel?;
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let p = self.parser.lock().ok()?;
        let end = (b.1 + 1).min(cols);
        let t = p.screen().contents_between(a.0, a.1, b.0, end);
        Some(t.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n"))
    }

    fn handle_input(&mut self, events: Vec<Event>) {
        let (app_cursor, bracketed) = match self.parser.lock() {
            Ok(p) => (p.screen().application_cursor(), p.screen().bracketed_paste()),
            Err(_) => (false, false),
        };
        let mut out: Vec<u8> = Vec::new();
        for ev in events {
            match ev {
                Event::Text(t) => out.extend_from_slice(t.as_bytes()),
                Event::Ime(egui::ImeEvent::Commit(t)) => {
                    self.preedit.clear();
                    out.extend_from_slice(t.as_bytes());
                }
                Event::Ime(egui::ImeEvent::Preedit(t)) => self.preedit = t,
                Event::Ime(egui::ImeEvent::Enabled) | Event::Ime(egui::ImeEvent::Disabled) => self.preedit.clear(),
                Event::Paste(t) => {
                    if bracketed {
                        out.extend_from_slice(b"\x1b[200~");
                        out.extend_from_slice(t.as_bytes());
                        out.extend_from_slice(b"\x1b[201~");
                    } else {
                        out.extend_from_slice(t.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
                    }
                }
                #[cfg(not(target_os = "macos"))]
                Event::Copy => out.push(3),
                #[cfg(not(target_os = "macos"))]
                Event::Cut => out.push(24),
                Event::Key { key: Key::PageUp, pressed: true, modifiers, .. } if modifiers.shift => {
                    let n = self.size.0 as i64 - 2;
                    self.scroll_lines(n.max(1));
                }
                Event::Key { key: Key::PageDown, pressed: true, modifiers, .. } if modifiers.shift => {
                    let n = self.size.0 as i64 - 2;
                    self.scroll_lines(-n.max(1));
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(b) = key_bytes(key, modifiers, app_cursor) {
                        out.extend_from_slice(&b);
                    }
                }
                _ => {}
            }
        }
        if !out.is_empty() {
            self.sel = None;
            if let Ok(mut p) = self.parser.lock() {
                p.screen_mut().set_scrollback(0);
            }
            self.write(&out);
        }
    }
}

fn key_bytes(k: Key, m: Modifiers, app: bool) -> Option<Vec<u8>> {
    if m.mac_cmd {
        return None;
    }
    let arrow = |c: char| if app { format!("\x1bO{c}") } else { format!("\x1b[{c}") };
    let s: String = match k {
        Key::Enter => if m.shift || m.alt { "\x1b\r".into() } else { "\r".into() },
        Key::Tab => if m.shift { "\x1b[Z".into() } else { "\t".into() },
        Key::Backspace => if m.alt { "\x1b\x7f".into() } else { "\x7f".into() },
        Key::Escape => "\x1b".into(),
        Key::ArrowUp => arrow('A'),
        Key::ArrowDown => arrow('B'),
        Key::ArrowRight => if m.alt { "\x1bf".into() } else { arrow('C') },
        Key::ArrowLeft => if m.alt { "\x1bb".into() } else { arrow('D') },
        Key::Home => "\x1b[H".into(),
        Key::End => "\x1b[F".into(),
        Key::PageUp => "\x1b[5~".into(),
        Key::PageDown => "\x1b[6~".into(),
        Key::Delete => "\x1b[3~".into(),
        Key::Insert => "\x1b[2~".into(),
        _ => {
            if m.ctrl {
                let n = k.name();
                if n.len() == 1 {
                    let ch = n.as_bytes()[0];
                    if ch.is_ascii_alphabetic() {
                        return Some(vec![ch.to_ascii_uppercase() - b'A' + 1]);
                    }
                }
            }
            return None;
        }
    };
    Some(s.into_bytes())
}


fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn idx(i: u8) -> Color32 {
    match i {
        0..=15 => hex(theme().ansi[i as usize]),
        16..=231 => {
            let i = i - 16;
            let lv = [0u8, 95, 135, 175, 215, 255];
            Color32::from_rgb(lv[(i / 36) as usize], lv[((i / 6) % 6) as usize], lv[(i % 6) as usize])
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

fn colors(cell: &vt100::Cell) -> (Color32, Color32) {
    let mut fg = match cell.fgcolor() {
        vt100::Color::Default => fg(),
        vt100::Color::Idx(i) => idx(if cell.bold() && i < 8 { i + 8 } else { i }),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    };
    let mut bg = match cell.bgcolor() {
        vt100::Color::Default => bg(),
        vt100::Color::Idx(i) => idx(i),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    };
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.dim() {
        fg = fg.gamma_multiply(0.6);
    }
    (fg, bg)
}

/// Caracteres que dibujamos a mano para que se vean perfectos sin depender de la fuente.
fn is_drawn(ch: char) -> bool {
    matches!(ch as u32, 0x2500..=0x257F | 0x2580..=0x259F | 0x2800..=0x28FF | 0x23BF)
}

fn draw_special(p: &egui::Painter, r: Rect, ch: char, col: Color32, sw: f32) {
    let u = ch as u32;
    let (x0, y0, w, h) = (r.min.x, r.min.y, r.width(), r.height());
    let fill = |x: f32, y: f32, fw: f32, fh: f32, c: Color32| {
        p.rect_filled(Rect::from_min_size(pos2(x0 + x * w, y0 + y * h), vec2(fw * w, fh * h)), 0.0, c);
    };
    match u {
        // Bloques
        0x2580 => fill(0.0, 0.0, 1.0, 0.5, col),
        0x2581..=0x2587 => {
            let f = (u - 0x2580) as f32 / 8.0;
            fill(0.0, 1.0 - f, 1.0, f, col)
        }
        0x2588 => fill(0.0, 0.0, 1.0, 1.0, col),
        0x2589..=0x258F => {
            let f = (0x2590 - u) as f32 / 8.0;
            fill(0.0, 0.0, f, 1.0, col)
        }
        0x2590 => fill(0.5, 0.0, 0.5, 1.0, col),
        0x2591 => fill(0.0, 0.0, 1.0, 1.0, col.gamma_multiply(0.25)),
        0x2592 => fill(0.0, 0.0, 1.0, 1.0, col.gamma_multiply(0.5)),
        0x2593 => fill(0.0, 0.0, 1.0, 1.0, col.gamma_multiply(0.75)),
        0x2594 => fill(0.0, 0.0, 1.0, 0.125, col),
        0x2595 => fill(0.875, 0.0, 0.125, 1.0, col),
        0x2596..=0x259F => {
            // cuadrantes: UL, UR, LL, LR
            let q: [bool; 4] = match u {
                0x2596 => [false, false, true, false],
                0x2597 => [false, false, false, true],
                0x2598 => [true, false, false, false],
                0x2599 => [true, false, true, true],
                0x259A => [true, false, false, true],
                0x259B => [true, true, true, false],
                0x259C => [true, true, false, true],
                0x259D => [false, true, false, false],
                0x259E => [false, true, true, false],
                _ => [false, true, true, true],
            };
            if q[0] { fill(0.0, 0.0, 0.5, 0.5, col) }
            if q[1] { fill(0.5, 0.0, 0.5, 0.5, col) }
            if q[2] { fill(0.0, 0.5, 0.5, 0.5, col) }
            if q[3] { fill(0.5, 0.5, 0.5, 0.5, col) }
        }
        // Braille
        0x2800..=0x28FF => {
            let bits = u - 0x2800;
            let dots = [(0, 0, 0x01), (0, 1, 0x02), (0, 2, 0x04), (1, 0, 0x08), (1, 1, 0x10), (1, 2, 0x20), (0, 3, 0x40), (1, 3, 0x80)];
            let rad = (w * 0.13).max(1.0);
            for (cx, cy, m) in dots {
                if bits & m != 0 {
                    p.circle_filled(pos2(x0 + w * (0.3 + 0.4 * cx as f32), y0 + h * (0.14 + 0.24 * cy as f32)), rad, col);
                }
            }
        }
        // ⎿ (Claude Code lo usa para las respuestas de herramientas)
        0x23BF => {
            let st = egui::Stroke::new(sw, col);
            let x = x0 + w * 0.35;
            let y = y0 + h * 0.6;
            p.line_segment([pos2(x, y0), pos2(x, y)], st);
            p.line_segment([pos2(x, y), pos2(x0 + w, y)], st);
        }
        // Líneas de caja
        0x2500..=0x257F => draw_box(p, r, u, col, sw),
        _ => {}
    }
}

fn draw_box(p: &egui::Painter, r: Rect, u: u32, col: Color32, sw: f32) {
    let c = r.center();
    let (l, rt, t, b) = (pos2(r.min.x, c.y), pos2(r.max.x, c.y), pos2(c.x, r.min.y), pos2(c.x, r.max.y));
    let light = egui::Stroke::new(sw, col);
    let heavy = egui::Stroke::new(sw * 2.0, col);
    // brazos: arriba, abajo, izquierda, derecha; 1 = fino, 2 = grueso
    let arms: [u8; 4] = match u {
        0x2500 | 0x2504 | 0x2508 | 0x254C => [0, 0, 1, 1],
        0x2501 | 0x2505 | 0x2509 | 0x254D => [0, 0, 2, 2],
        0x2502 | 0x2506 | 0x250A | 0x254E => [1, 1, 0, 0],
        0x2503 | 0x2507 | 0x250B | 0x254F => [2, 2, 0, 0],
        0x250C | 0x256D => [0, 1, 0, 1],
        0x250F => [0, 2, 0, 2],
        0x2510 | 0x256E => [0, 1, 1, 0],
        0x2513 => [0, 2, 2, 0],
        0x2514 | 0x2570 => [1, 0, 0, 1],
        0x2517 => [2, 0, 0, 2],
        0x2518 | 0x256F => [1, 0, 1, 0],
        0x251B => [2, 0, 2, 0],
        0x251C => [1, 1, 0, 1],
        0x2523 => [2, 2, 0, 2],
        0x2524 => [1, 1, 1, 0],
        0x252B => [2, 2, 2, 0],
        0x252C => [0, 1, 1, 1],
        0x2533 => [0, 2, 2, 2],
        0x2534 => [1, 0, 1, 1],
        0x253B => [2, 0, 2, 2],
        0x253C => [1, 1, 1, 1],
        0x254B => [2, 2, 2, 2],
        0x2574 => [0, 0, 1, 0],
        0x2575 => [1, 0, 0, 0],
        0x2576 => [0, 0, 0, 1],
        0x2577 => [0, 1, 0, 0],
        0x2578 => [0, 0, 2, 0],
        0x2579 => [2, 0, 0, 0],
        0x257A => [0, 0, 0, 2],
        0x257B => [0, 2, 0, 0],
        0x2550 => {
            let d = sw * 1.5;
            p.line_segment([l - vec2(0.0, d), rt - vec2(0.0, d)], light);
            p.line_segment([l + vec2(0.0, d), rt + vec2(0.0, d)], light);
            return;
        }
        0x2551 => {
            let d = sw * 1.5;
            p.line_segment([t - vec2(d, 0.0), b - vec2(d, 0.0)], light);
            p.line_segment([t + vec2(d, 0.0), b + vec2(d, 0.0)], light);
            return;
        }
        _ => {
            // resto: aproximación como cruz fina según forma desconocida
            [0, 0, 1, 1]
        }
    };
    let rounded = matches!(u, 0x256D..=0x2570);
    if rounded {
        let ends: Vec<egui::Pos2> = [(arms[0], t), (arms[1], b), (arms[2], l), (arms[3], rt)]
            .iter()
            .filter(|(a, _)| *a > 0)
            .map(|(_, p)| *p)
            .collect();
        if ends.len() == 2 {
            p.add(egui::epaint::QuadraticBezierShape::from_points_stroke([ends[0], c, ends[1]], false, Color32::TRANSPARENT, light));
        }
        return;
    }
    let dashed = matches!(u, 0x2504..=0x250B | 0x254C..=0x254F);
    for (a, end) in [(arms[0], t), (arms[1], b), (arms[2], l), (arms[3], rt)] {
        if a == 0 {
            continue;
        }
        let st = if a == 2 { heavy } else { light };
        if dashed {
            let n = 3;
            for i in 0..n {
                let f0 = i as f32 / n as f32;
                let f1 = f0 + 0.6 / n as f32;
                p.line_segment([c + (end - c) * f0, c + (end - c) * f1], st);
            }
        } else {
            p.line_segment([c, end], st);
        }
    }
}
