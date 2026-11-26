#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod i18n;
mod app;
mod bots;
mod claude;
mod config;
mod git;
mod ssh;
mod term;
mod updater;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = bots::cli(&args) {
        std::process::exit(code);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("tonycode")
            .with_inner_size([1320.0, 840.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("tonycode", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn pty_runs_and_parses() {
        let ctx = egui::Context::default();
        let t = crate::term::Term::spawn(
            "/bin/sh",
            &["-c".into(), "printf '\\033[31mhola\\033[0m tonycode'; echo $CLAUDE_CONFIG_DIR".into()],
            std::path::Path::new("/tmp"),
            &[("CLAUDE_CONFIG_DIR".into(), "/x/.claude-work".into())],
            ctx,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(600));
        let s = t.screen_text();
        assert!(s.contains("hola tonycode"), "{s}");
        assert!(s.contains("/x/.claude-work"), "{s}");
        let p = t.parser.lock().unwrap();
        assert_eq!(p.screen().cell(0, 0).unwrap().fgcolor(), vt100::Color::Idx(1));
    }

    #[test]
    fn tracker_reads_agents() {
        let dir = std::env::temp_dir().join("tc-test");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.jsonl");
        std::fs::write(&f, concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Task","input":{"subagent_type":"Explore","description":"Buscar usos"}}]}}"#, "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"Agent","input":{"description":"Migrar tests"}}]}}"#, "\n",
        )).unwrap();
        let mut tr = crate::claude::Tracker::default();
        tr.poll(&f);
        assert_eq!(tr.agents.len(), 2);
        assert_eq!(tr.running(), 2);
        use std::io::Write;
        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        writeln!(h, r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}"#).unwrap();
        tr.poll(&f);
        assert_eq!(tr.running(), 1);
        assert_eq!(tr.agents[0].kind, "Explore");
    }

    #[test]
    fn sanitize_matches_claude() {
        assert_eq!(crate::claude::sanitize("/Users/oswaldo/dev/my_app.v2"), "-Users-oswaldo-dev-my-app-v2");
    }
}

#[cfg(test)]
mod ssh_test {
    #[test]
    fn parses_github_hosts() {
        let home = std::env::temp_dir().join("tc-ssh-home");
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh/config"), "Host github.com\n  HostName github.com\n  IdentityFile ~/.ssh/github-personal\n\nHost github.com-aceable\n  HostName github.com\n  IdentityFile ~/.ssh/github-aceable\n\nHost github.com-devninjasource\n  HostName github.com\n  IdentityFile ~/.ssh/github-devninjasource\n\nHost github.com-oswaldoninjasource\n  HostName github.com\n  IdentityFile ~/.ssh/github-oswaldoninjasource\n\nHost mayan-monitor\n  HostName 98.93.115.73\n  IdentityFile ~/.ssh/monitoring-lightsail.pem\n").unwrap();
        unsafe { std::env::set_var("HOME", &home); }
        let h = crate::ssh::github_hosts();
        let labels: Vec<String> = h.iter().map(|x| format!("{}={}", x.label(), x.key)).collect();
        println!("{labels:?}");
        assert_eq!(h.len(), 4);
        assert!(labels.contains(&"aceable=~/.ssh/github-aceable".to_string()));
        assert!(labels.contains(&"personal=~/.ssh/github-personal".to_string()));
        assert!(!labels.iter().any(|l| l.contains("lightsail")));
    }
}
