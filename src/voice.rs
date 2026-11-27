//! Modo voz: hooks de Claude Code (Stop / Notification) y lectura en voz alta con `say` de macOS.

use crate::config;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command};

/// Evento que deja el hook en ~/.tonycode/events para que la app lo procese.
#[derive(Serialize, Deserialize)]
pub struct HookEvent {
    pub chat: String,
    pub event: String,
    #[serde(default)]
    pub text: String,
}

pub fn events_dir() -> PathBuf {
    config::app_dir().join("events")
}

/// `tonycode hook`: lo ejecuta Claude Code. Lee el JSON del hook por stdin y deja un evento.
/// Nunca falla ni imprime nada, para no interferir con la sesión.
pub fn hook_cli() -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let chat = std::env::var("TONYCODE_CHAT").unwrap_or_default();
    if chat.is_empty() {
        return 0;
    }
    let v: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    let event = v["hook_event_name"].as_str().unwrap_or("").to_string();
    let text = match event.as_str() {
        "Stop" => v["last_assistant_message"].as_str().unwrap_or("").to_string(),
        _ => v["message"].as_str().unwrap_or("").to_string(),
    };
    let ev = HookEvent { chat, event, text };
    let dir = events_dir();
    let _ = std::fs::create_dir_all(&dir);
    let id = uuid::Uuid::new_v4();
    let tmp = dir.join(format!("{id}.tmp"));
    if std::fs::write(&tmp, serde_json::to_string(&ev).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join(format!("{id}.json")));
    }
    0
}

/// Archivos que se pasan a claude cuando el modo voz está activo:
/// settings con los hooks y el texto que se agrega al system prompt.
pub fn prepare_files(lang_en: bool) -> Option<(PathBuf, PathBuf)> {
    let dir = config::app_dir().join("voice");
    std::fs::create_dir_all(&dir).ok()?;
    let exe = std::env::current_exe().ok()?;
    let cmd = format!("'{}' hook", exe.display());
    let settings = serde_json::json!({
        "hooks": {
            "Stop": [{ "hooks": [{ "type": "command", "command": cmd, "timeout": 10 }] }],
            "Notification": [{ "matcher": "permission_prompt", "hooks": [{ "type": "command", "command": cmd, "timeout": 10 }] }]
        }
    });
    let sp = dir.join("hooks-settings.json");
    std::fs::write(&sp, serde_json::to_string_pretty(&settings).ok()?).ok()?;
    let prompt = if lang_en {
        "When you finish responding, end your reply with one line that starts with 'Summary:' containing one or two short sentences in natural spoken language (no code, paths or symbols) saying what you did or what you need from the user. That line will be read aloud."
    } else {
        "Cuando termines de responder, agrega al final una línea que empiece con 'Resumen:' con una o dos frases cortas, en lenguaje hablado y natural (sin código, rutas ni símbolos), que diga qué hiciste o qué necesitas del usuario. Esa línea se leerá en voz alta."
    };
    let pp = dir.join(if lang_en { "append-prompt-en.txt" } else { "append-prompt-es.txt" });
    std::fs::write(&pp, prompt).ok()?;
    Some((sp, pp))
}

/// Saca un resumen corto para leer en voz alta.
pub fn spoken_summary(text: &str) -> String {
    // 1) La línea "Resumen:" / "Summary:" que le pedimos a Claude.
    for line in text.lines().rev() {
        let clean = line.trim().trim_start_matches(['*', '_', '>', '#', ' ', '-']);
        let low = clean.to_lowercase();
        for key in ["resumen:", "summary:"] {
            if let Some(pos) = low.find(key) {
                if pos < 4 {
                    let rest = &clean[pos + key.len()..];
                    let s = strip_md(rest);
                    if !s.is_empty() {
                        return limit(&s, 320);
                    }
                }
            }
        }
    }
    // 2) Si no, las primeras frases del texto, sin bloques de código.
    let mut out = String::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || line.trim().is_empty() || line.trim_start().starts_with('|') {
            continue;
        }
        out.push_str(&strip_md(line));
        out.push(' ');
        if out.len() > 400 {
            break;
        }
    }
    let mut sentences = String::new();
    let mut count = 0;
    for ch in out.chars() {
        sentences.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            count += 1;
            if count >= 2 {
                break;
            }
        }
    }
    limit(sentences.trim(), 260)
}

fn strip_md(s: &str) -> String {
    let mut r = String::new();
    let mut prev_space = false;
    for ch in s.chars() {
        let c = match ch {
            '*' | '_' | '`' | '#' | '>' | '|' | '~' => continue,
            '\t' => ' ',
            c => c,
        };
        if c == ' ' {
            if prev_space {
                continue;
            }
            prev_space = true;
        } else {
            prev_space = false;
        }
        r.push(c);
    }
    // quita URLs largas
    r.split(' ').filter(|w| !w.starts_with("http")).collect::<Vec<_>>().join(" ").trim().to_string()
}

fn limit(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    match cut.rfind(' ') {
        Some(i) => format!("{}…", &cut[..i]),
        None => cut,
    }
}

/// Voces instaladas del sistema para un idioma ("es" / "en"): (nombre, locale).
pub fn voices(lang: &str) -> Vec<(String, String)> {
    let Ok(o) = Command::new("say").args(["-v", "?"]).output() else { return vec![] };
    let txt = String::from_utf8_lossy(&o.stdout);
    let mut v = Vec::new();
    for line in txt.lines() {
        // "Paulina             es_MX    # Hola, me llamo Paulina..."
        let head = line.split('#').next().unwrap_or("").trim_end();
        let mut parts: Vec<&str> = head.split_whitespace().collect();
        let Some(locale) = parts.pop() else { continue };
        if !locale.starts_with(&format!("{lang}_")) {
            continue;
        }
        let name = parts.join(" ");
        if !name.is_empty() {
            v.push((name, locale.to_string()));
        }
    }
    v
}

/// Habla un texto (cancela lo que se esté diciendo). Devuelve el proceso para poder detenerlo.
pub fn speak(text: &str, voice: &str, rate: u32) -> Option<Child> {
    if text.trim().is_empty() {
        return None;
    }
    if let Ok(log) = std::env::var("TONYCODE_SAY_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
            let _ = writeln!(f, "[{voice}|{rate}] {text}");
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = Command::new("say");
        if !voice.is_empty() {
            c.args(["-v", voice]);
        }
        c.args(["-r", &rate.to_string(), text]);
        return c.spawn().ok();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (voice, rate);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::spoken_summary;

    #[test]
    fn picks_summary_line() {
        let t = "Hice cambios en `src/a.rs`.\n\n```rust\nfn x(){}\n```\n\n**Resumen:** Arreglé el bug de agencias y los tests pasan.";
        assert_eq!(spoken_summary(t), "Arreglé el bug de agencias y los tests pasan.");
    }

    #[test]
    fn falls_back_to_first_sentences() {
        let t = "## Listo\nActualicé el módulo de pagos. Los tests pasan. Falta revisar el deploy.\n```\ncode\n```";
        assert_eq!(spoken_summary(t), "Listo Actualicé el módulo de pagos. Los tests pasan.");
    }
}
