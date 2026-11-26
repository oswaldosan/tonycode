//! Lee ~/.ssh/config y devuelve solo los hosts de GitHub.

#[derive(Clone)]
pub struct GhHost {
    pub alias: String,
    pub key: String,
}

impl GhHost {
    pub fn label(&self) -> String {
        match self.alias.strip_prefix("github.com-") {
            Some(r) => r.to_string(),
            None if self.alias == "github.com" => {
                let file = self.key.rsplit('/').next().unwrap_or("");
                let name = file.trim_start_matches("github-").trim_start_matches("id_");
                if name.is_empty() { "default".into() } else { name.to_string() }
            }
            None => self.alias.clone(),
        }
    }
}

pub fn github_hosts() -> Vec<GhHost> {
    let path = crate::config::home().join(".ssh").join("config");
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    let mut out = Vec::new();
    let mut cur: Option<(String, String, String)> = None; // alias, hostname, key
    let flush = |c: Option<(String, String, String)>, out: &mut Vec<GhHost>| {
        if let Some((alias, hn, key)) = c {
            let is_gh = hn.eq_ignore_ascii_case("github.com") || (hn.is_empty() && alias.starts_with("github.com"));
            if is_gh && !alias.contains('*') {
                out.push(GhHost { alias, key });
            }
        }
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim().trim_start_matches('=').trim().to_string()),
            None => continue,
        };
        match k.as_str() {
            "host" => {
                flush(cur.take(), &mut out);
                let alias = v.split_whitespace().next().unwrap_or("").to_string();
                cur = Some((alias, String::new(), String::new()));
            }
            "hostname" => {
                if let Some(c) = cur.as_mut() {
                    c.1 = v;
                }
            }
            "identityfile" => {
                if let Some(c) = cur.as_mut() {
                    c.2 = v;
                }
            }
            _ => {}
        }
    }
    flush(cur.take(), &mut out);
    out
}
