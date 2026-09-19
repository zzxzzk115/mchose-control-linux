//! Desktop lifecycle preferences, single instance and native tray bridge.
use std::{
    fs, io,
    io::BufRead,
    os::fd::AsRawFd,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
};
fn config() -> PathBuf {
    crate::preset::path().with_file_name("desktop.conf")
}
pub fn close_to_tray() -> bool {
    fs::read_to_string(config()).map_or(true, |s| s.trim() != "close_to_tray=false")
}
pub fn set_close_to_tray(value: bool) -> io::Result<()> {
    let p = config();
    fs::create_dir_all(p.parent().unwrap())?;
    fs::write(p, format!("close_to_tray={value}\n"))
}
pub fn autostart_path() -> PathBuf {
    crate::auto::autostart_path().with_file_name("mchose-gui.desktop")
}
pub fn set_autostart(value: bool) -> io::Result<()> {
    let p = autostart_path();
    if !value {
        return match fs::remove_file(p) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        };
    }
    let exe = std::env::current_exe()?.with_file_name("mchose-gui");
    let exe = exe
        .to_string_lossy()
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('$', "\\\\$")
        .replace('`', "\\\\`")
        .replace('%', "%%");
    fs::create_dir_all(p.parent().unwrap())?;
    fs::write(p,format!("[Desktop Entry]\nType=Application\nName=MCHOSE Mouse\nName[zh_CN]=迈从鼠标\nExec=\"{exe}\" --background\nIcon=mchose\nTerminal=false\n"))
}
fn runtime() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("mchose-{}", unsafe { libc::geteuid() }))
        })
        .join("mchose")
}
pub struct Instance {
    _lock: fs::File,
}
impl Instance {
    pub fn acquire() -> io::Result<Option<Self>> {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(runtime())?;
        fs::set_permissions(runtime(), fs::Permissions::from_mode(0o700))?;
        let mut f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(runtime().join("gui.lock"))?;
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            fs::write(runtime().join("gui-show"), b"show")?;
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                if let Ok(pid) = fs::read_to_string(runtime().join("gui.lock"))
                    .unwrap_or_default()
                    .trim()
                    .parse::<u32>()
                {
                    window(pid, false)?;
                }
            }
            return Ok(None);
        }
        use std::io::Write;
        f.set_len(0)?;
        write!(f, "{}", std::process::id())?;
        let _ = fs::remove_file(runtime().join("gui-show"));
        Ok(Some(Self { _lock: f }))
    }
}
pub fn take_show() -> bool {
    fs::remove_file(runtime().join("gui-show")).is_ok()
}
pub struct Tray {
    child: Child,
    pub events: Receiver<String>,
}
impl Tray {
    pub fn start(zh: bool) -> io::Result<Self> {
        let mut child = Command::new("python3")
            .arg("-c")
            .arg(include_str!("../integrations/tray.py"))
            .arg(if zh { "zh" } else { "en" })
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in io::BufReader::new(stdout).lines().map_while(Result::ok) {
                if matches!(line.as_str(), "start" | "stop") {
                    let result = if line == "start" {
                        crate::auto::start()
                    } else {
                        crate::auto::stop()
                    };
                    if result.is_ok() {
                        continue;
                    }
                }
                if line != "ready" && std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    let _ = window(std::process::id(), false);
                }
                if tx.send(line).is_err() {
                    return;
                }
            }
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                let _ = window(std::process::id(), false);
            }
            let _ = tx.send("unavailable".into());
        });
        Ok(Self { child, events })
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Winit cannot hide or unminimize Wayland surfaces. KWin handles only this GUI's PID.
pub struct WindowControl {
    tx: std::sync::mpsc::Sender<bool>,
    pub errors: Receiver<String>,
}
impl WindowControl {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<bool>();
        let (error_tx, errors) = mpsc::channel();
        let pid = std::process::id();
        std::thread::spawn(move || {
            for hide in rx {
                if let Err(e) = window(pid, hide) {
                    let _ = error_tx.send(e.to_string());
                }
            }
        });
        Self { tx, errors }
    }
    pub fn set_hidden(&self, hide: bool) {
        let _ = self.tx.send(hide);
    }
}
impl Default for WindowControl {
    fn default() -> Self {
        Self::new()
    }
}

fn window(pid: u32, hide: bool) -> io::Result<()> {
    let output = Command::new("python3")
        .arg("-c")
        .arg(include_str!("../integrations/window.py"))
        .arg(pid.to_string())
        .arg(if hide { "hide" } else { "show" })
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(())
}
