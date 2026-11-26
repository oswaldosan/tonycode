//! Revisa si hay commits nuevos en el repo de tonycode y permite actualizar con un clic.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const BUILT_COMMIT: &str = env!("TONYCODE_COMMIT");

#[derive(Clone, Default)]
pub enum Upd {
    #[default]
    Unknown,
    NoRepo,
    UpToDate,
    Available(Vec<String>),
    Error(String),
}

pub fn source_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn short_commit() -> String {
    BUILT_COMMIT.chars().take(7).collect()
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("git").args(args).current_dir(dir).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// Consulta el remoto. Hay actualización si el remoto tiene commits que este binario no incluye.
pub fn check() -> Upd {
    let dir = source_dir();
    if !dir.join(".git").exists() {
        return Upd::NoRepo;
    }
    if git(&dir, &["rev-parse", "--abbrev-ref", "@{u}"]).is_err() {
        return Upd::NoRepo;
    }
    if let Err(e) = git(&dir, &["fetch", "--quiet"]) {
        return Upd::Error(format!("git fetch falló: {e}"));
    }
    let base = if BUILT_COMMIT.is_empty() { "HEAD".to_string() } else { BUILT_COMMIT.to_string() };
    match git(&dir, &["log", "--oneline", "--no-decorate", &format!("{base}..@{{u}}")]) {
        Ok(out) if out.is_empty() => Upd::UpToDate,
        Ok(out) => Upd::Available(out.lines().map(|l| l.to_string()).collect()),
        Err(e) => Upd::Error(e),
    }
}

/// Hilo que revisa al iniciar y cada 30 minutos.
pub fn spawn_checker(state: Arc<Mutex<Upd>>, ctx: egui::Context) {
    std::thread::spawn(move || loop {
        let r = check();
        if let Ok(mut s) = state.lock() {
            *s = r;
        }
        ctx.request_repaint();
        std::thread::sleep(Duration::from_secs(30 * 60));
    });
}

/// Comando que actualiza: pull, compila e instala. Imprime una marca al terminar bien.
pub fn update_command() -> (String, Vec<String>) {
    let sh = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let dir = source_dir();
    let line = format!(
        "cd '{}' && git pull --ff-only && ./install.sh --no-open && echo && echo TONYCODE_UPDATE_OK",
        dir.display()
    );
    (sh, vec!["-lc".into(), line])
}

/// Relanza la app instalada y sale.
pub fn relaunch() {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("sh").args(["-c", "sleep 1; open \"$HOME/Applications/tonycode.app\""]).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new("sh").args(["-c", &format!("sleep 1; '{}' &", exe.display())]).spawn();
        }
    }
    std::process::exit(0);
}
