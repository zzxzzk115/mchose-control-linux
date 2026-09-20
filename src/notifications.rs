//! Passive notifications on a separate worker; event history survives daemon exits.
use std::{
    fs, io,
    process::Command,
    sync::mpsc::{self, SyncSender},
    thread::JoinHandle,
};
pub fn enabled() -> bool {
    fs::read_to_string(crate::preset::path().with_file_name("notifications"))
        .map_or(true, |s| s.trim() != "off")
}
pub fn set_enabled(value: bool) -> io::Result<()> {
    let p = crate::preset::path().with_file_name("notifications");
    fs::create_dir_all(p.parent().unwrap())?;
    fs::write(p, if value { "on" } else { "off" })
}
pub fn send(title: &str, body: &str) -> io::Result<()> {
    let out = Command::new("python3")
        .arg("-c")
        .arg(include_str!("../integrations/notify.py"))
        .arg(title)
        .arg(body)
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ));
    }
    Ok(())
}
pub fn history() -> String {
    fs::read_to_string(crate::preset::path().with_file_name("events.log")).unwrap_or_default()
}
pub struct Notifier {
    tx: Option<SyncSender<(String, String)>>,
    worker: Option<JoinHandle<()>>,
}
impl Notifier {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel::<(String, String)>(8);
        let worker = std::thread::spawn(move || {
            for (title, body) in rx {
                if let Err(e) = send(&title, &body) {
                    eprintln!("desktop notification: {e}");
                }
            }
        });
        Self {
            tx: Some(tx),
            worker: Some(worker),
        }
    }
    pub fn event(&self, title: &str, body: &str) {
        let timestamp = crate::log::stamp();
        let line = format!(
            "{timestamp}\t{}\t{}",
            title.replace(['\n', '\r', '\t'], " "),
            body.replace(['\n', '\r', '\t'], " ")
        );
        let previous = history();
        let mut lines: Vec<_> = previous.lines().rev().take(99).collect();
        lines.reverse();
        let mut text = lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&line);
        text.push('\n');
        let p = crate::preset::path().with_file_name("events.log");
        let _ = fs::create_dir_all(p.parent().unwrap());
        let tmp = p.with_extension(format!("{}.tmp", std::process::id()));
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(tmp, p);
        }
        eprintln!("{line}");
        if enabled() {
            if let Some(tx) = &self.tx {
                let _ = tx.try_send((title.into(), body.into()));
            }
        }
    }
}
impl Drop for Notifier {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
