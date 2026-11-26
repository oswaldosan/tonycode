use std::path::Path;
use std::process::Command;

pub fn run(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("no se pudo ejecutar git: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

pub fn branch(cwd: &Path) -> String {
    run(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn is_repo(cwd: &Path) -> bool {
    run(cwd, &["rev-parse", "--is-inside-work-tree"]).is_ok()
}
