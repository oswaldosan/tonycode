//! Terminal embebida: PTY real + parser vt100, dibujada con egui.

use egui::{Color32, Event, FontId, Key, Modifiers, Rect, Sense, pos2, vec2};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const BG: Color32 = Color32::from_rgb(0x0D, 0x0E, 0x10);
pub const FG: Color32 = Color32::from_rgb(0xD7, 0xDA, 0xE0);

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

    pub fn ui(&mut self, ui: &mut egui::Ui, font: &FontId, input: Vec<Event>, focused: bool) -> egui::Response {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click());
        let (cw, rh) = ui.fonts(|f| (f.glyph_width(font, 'M'), f.row_height(font)));
        let pad = 10.0;
        let cols = (((rect.width() - pad * 2.0) / cw).floor() as u16).max(20);
        let rows = (((rect.height() - pad * 2.0) / rh).floor() as u16).max(5);
        self.resize(rows, cols);
        if !input.is_empty() {
            self.handle_input(input);
        }

        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                self.scroll_acc += dy / rh;
                let lines = self.scroll_acc.trunc() as i64;
                if lines != 0 {
                    self.scroll_acc -= lines as f32;
                    if let Ok(mut p) = self.parser.lock() {
                        let cur = p.screen().scrollback() as i64;
                        p.screen_mut().set_scrollback((cur + lines).max(0) as usize);
                    }
                }
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BG);
        let Ok(p) = self.parser.lock() else { return resp };
        let s = p.screen();
        let origin = rect.min + vec2(pad, pad);
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
                if t.is_empty() || t.is_ascii() {
                    text.push_str(if t.is_empty() { " " } else { t });
                    c += 1;
                    while c < cols {
                        let Some(n) = s.cell(r, c) else { break };
                        let nt = n.contents();
                        if n.is_wide_continuation() || !(nt.is_empty() || nt.is_ascii()) || colors(n) != (fg, bg) {
                            break;
                        }
                        text.push_str(if nt.is_empty() { " " } else { nt });
                        c += 1;
                    }
                } else {
                    text.push_str(t);
                    c += if cell.is_wide() { 2 } else { 1 };
                }
                let x = origin.x + start as f32 * cw;
                if bg != BG {
                    let w = (c - start) as f32 * cw;
                    painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, rh)), 0.0, bg);
                }
                if !text.trim().is_empty() {
                    painter.text(pos2(x, y), egui::Align2::LEFT_TOP, text, font.clone(), fg);
                }
            }
        }
        if !s.hide_cursor() && s.scrollback() == 0 {
            let (cr, cc) = s.cursor_position();
            let r = Rect::from_min_size(origin + vec2(cc as f32 * cw, cr as f32 * rh), vec2(cw, rh));
            if focused {
                painter.rect_filled(r, 1.0, Color32::from_rgba_unmultiplied(0xD7, 0xDA, 0xE0, 150));
            } else {
                painter.rect_stroke(r, 1.0, egui::Stroke::new(1.0_f32, Color32::from_gray(110)), egui::StrokeKind::Inside);
            }
        }
        if s.scrollback() > 0 {
            painter.text(
                rect.right_top() + vec2(-12.0, 8.0),
                egui::Align2::RIGHT_TOP,
                format!("↑ {} líneas", s.scrollback()),
                FontId::proportional(11.0),
                Color32::from_gray(140),
            );
        }
        resp
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
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(b) = key_bytes(key, modifiers, app_cursor) {
                        out.extend_from_slice(&b);
                    }
                }
                _ => {}
            }
        }
        if !out.is_empty() {
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

const PAL16: [u32; 16] = [
    0x1E2024, 0xE06C75, 0x98C379, 0xE5C07B, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xABB2BF,
    0x5C6370, 0xF07178, 0xB5E08A, 0xF0C060, 0x8AB4FF, 0xD7A6F0, 0x7FD8D0, 0xE6E6E6,
];

fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn idx(i: u8) -> Color32 {
    match i {
        0..=15 => hex(PAL16[i as usize]),
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
        vt100::Color::Default => FG,
        vt100::Color::Idx(i) => idx(if cell.bold() && i < 8 { i + 8 } else { i }),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    };
    let mut bg = match cell.bgcolor() {
        vt100::Color::Default => BG,
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
