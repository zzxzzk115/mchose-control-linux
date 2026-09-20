//! Foreground-app rules, glob filters and a single background preset controller.
use crate::{
    hidraw::{self, HidRaw},
    preset::{self, Preset},
    proto::{self, Config},
};
use std::{
    fs, io,
    io::BufRead,
    os::unix::{fs::PermissionsExt, io::AsRawFd, process::CommandExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
fn invalid(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}
#[derive(Clone, Default, Debug)]
pub struct Window {
    pub app_id: String,
    pub class: String,
    pub exe: String,
    pub path: String,
    pub title: String,
}
#[derive(Clone, Debug)]
enum Expr {
    Field(String, String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}
#[derive(Clone, Debug)]
enum Token {
    Atom(String),
    And,
    Or,
    Not,
    Left,
    Right,
}
fn tokens(input: &str) -> io::Result<Vec<Token>> {
    if input.len() > 1024 {
        return Err(invalid("filter is too long"));
    }
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '&' | '|' => {
                if chars.next() != Some(c) {
                    return Err(invalid("use && or ||"));
                }
                out.push(if c == '&' { Token::And } else { Token::Or });
            }
            '!' => out.push(Token::Not),
            '(' => out.push(Token::Left),
            ')' => out.push(Token::Right),
            _ => {
                let mut atom = String::new();
                let mut quoted = false;
                let mut current = Some(c);
                while let Some(c) = current {
                    if c == '"' {
                        quoted = !quoted;
                    } else if c == '\\' && quoted {
                        atom.push(chars.next().ok_or_else(|| invalid("incomplete escape"))?);
                    } else {
                        atom.push(c);
                    }
                    match chars.peek() {
                        Some(c) if quoted || (!c.is_whitespace() && !"&|!()".contains(*c)) => {
                            current = chars.next()
                        }
                        _ => break,
                    }
                }
                if quoted {
                    return Err(invalid("unclosed quote"));
                }
                out.push(Token::Atom(atom));
            }
        }
    }
    Ok(out)
}
struct Parser {
    tokens: Vec<Token>,
    index: usize,
}
impl Parser {
    fn primary(&mut self) -> io::Result<Expr> {
        let token = self
            .tokens
            .get(self.index)
            .cloned()
            .ok_or_else(|| invalid("missing filter term"))?;
        self.index += 1;
        match token {
            Token::Not => Ok(Expr::Not(Box::new(self.primary()?))),
            Token::Left => {
                let e = self.or()?;
                if !matches!(self.tokens.get(self.index), Some(Token::Right)) {
                    return Err(invalid("missing )"));
                }
                self.index += 1;
                Ok(e)
            }
            Token::Atom(a) => {
                let (k, v) = a
                    .split_once('=')
                    .ok_or_else(|| invalid("use field=glob, e.g. app_id=steam_app_730"))?;
                if !["app_id", "class", "exe", "path", "title"].contains(&k) || v.is_empty() {
                    return Err(invalid(
                        "fields: app_id, class, exe, path, title; pattern cannot be empty",
                    ));
                }
                Ok(Expr::Field(k.into(), v.into()))
            }
            _ => Err(invalid("unexpected filter operator")),
        }
    }
    fn and(&mut self) -> io::Result<Expr> {
        let mut e = self.primary()?;
        while matches!(self.tokens.get(self.index), Some(Token::And)) {
            self.index += 1;
            e = Expr::And(Box::new(e), Box::new(self.primary()?));
        }
        Ok(e)
    }
    fn or(&mut self) -> io::Result<Expr> {
        let mut e = self.and()?;
        while matches!(self.tokens.get(self.index), Some(Token::Or)) {
            self.index += 1;
            e = Expr::Or(Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }
}
fn parse(s: &str) -> io::Result<Expr> {
    let mut p = Parser {
        tokens: tokens(s)?,
        index: 0,
    };
    let e = p.or()?;
    if p.index != p.tokens.len() {
        return Err(invalid("unexpected filter term"));
    }
    Ok(e)
}
pub fn validate_filter(s: &str) -> io::Result<()> {
    parse(s).map(|_| ())
}
fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut i, mut j, mut star, mut retry) = (0, 0, None, 0);
    while j < t.len() {
        if i < p.len() && (p[i] == '?' || p[i] == t[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            i += 1;
            retry = j;
        } else if let Some(s) = star {
            retry += 1;
            j = retry;
            i = s + 1;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}
impl Expr {
    fn matches(&self, w: &Window) -> bool {
        match self {
            Self::Field(k, p) => glob(
                p,
                match k.as_str() {
                    "app_id" => &w.app_id,
                    "class" => &w.class,
                    "exe" => &w.exe,
                    "path" => &w.path,
                    _ => &w.title,
                },
            ),
            Self::Not(e) => !e.matches(w),
            Self::And(a, b) => a.matches(w) && b.matches(w),
            Self::Or(a, b) => a.matches(w) || b.matches(w),
        }
    }
}
pub fn matches(filter: &str, w: &Window) -> io::Result<bool> {
    Ok(parse(filter)?.matches(w))
}
#[derive(Clone, Debug)]
pub struct Rule {
    pub enabled: bool,
    pub preset: String,
    pub filter: String,
}
pub fn rules_path() -> PathBuf {
    preset::path().with_file_name("app-rules.conf")
}
pub fn rules() -> io::Result<Vec<Rule>> {
    let text = match fs::read_to_string(rules_path()) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut rules = Vec::new();
    for line in text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<_> = line.splitn(3, '\t').collect();
        if fields.len() != 3 {
            return Err(invalid("invalid app-rules.conf line"));
        }
        validate_filter(fields[2])?;
        preset::validate_name(fields[1])?;
        if !["0", "1"].contains(&fields[0]) {
            return Err(invalid("invalid rule enabled value"));
        }
        rules.push(Rule {
            enabled: fields[0] == "1",
            preset: preset::canonical_name(fields[1]).into(),
            filter: fields[2].into(),
        });
    }
    Ok(rules)
}
pub fn save_rules(rules: &[Rule]) -> io::Result<()> {
    let mut text =
        String::from("# enabled<TAB>preset<TAB>filter; first matching enabled rule wins\n");
    for r in rules {
        validate_filter(&r.filter)?;
        preset::validate_name(&r.preset)?;
        if r.filter.contains(['\n', '\r', '\t']) {
            return Err(invalid("rule must be one line"));
        }
        text.push_str(&format!(
            "{}\t{}\t{}\n",
            r.enabled as u8, r.preset, r.filter
        ));
    }
    atomic_write(&rules_path(), text.as_bytes())
}
pub fn selected(rules: &[Rule], window: &Window) -> Option<usize> {
    if window.app_id.is_empty()
        && window.class.is_empty()
        && window.exe.is_empty()
        && window.title.is_empty()
    {
        return None;
    }
    rules
        .iter()
        .position(|r| r.enabled && matches(&r.filter, window).unwrap_or(false))
}
fn runtime() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("mchose-{}", unsafe { libc::geteuid() }))
        })
        .join("mchose")
}
fn prepare() -> io::Result<()> {
    let p = runtime();
    fs::create_dir_all(&p)?;
    fs::set_permissions(p, fs::Permissions::from_mode(0o700))
}
fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}
fn lock() -> io::Result<fs::File> {
    prepare()?;
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(runtime().join("auto.lock"))?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "automatic switching is already running",
        ));
    }
    Ok(f)
}
pub fn running() -> bool {
    lock().is_err_and(|e| e.kind() == io::ErrorKind::AlreadyExists)
}
pub fn start() -> io::Result<()> {
    if running() {
        return Ok(());
    }
    prepare()?;
    let _ = fs::remove_file(runtime().join("stop"));
    let exe = std::env::current_exe()?.with_file_name("mchose");
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(runtime().join("auto.log"))?;
    let mut command = Command::new(exe);
    // A login launcher or terminal may exit immediately after starting us.
    // setsid is async-signal-safe and detaches the controller's session.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .args(["auto", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()?;
    Ok(())
}
pub fn stop() -> io::Result<()> {
    prepare()?;
    fs::write(runtime().join("stop"), b"stop")
}
#[derive(Clone, Default)]
pub struct Status {
    pub revision: u64,
    pub last_event: String,
    pub state: String,
    pub active: String,
    pub message: String,
    pub window: Window,
    pub last_app: Window,
}
fn clean(s: &str) -> String {
    s.replace(['\n', '\r', '\t'], " ")
}
fn write_status(s: &Status) {
    let revision = s.revision.to_string();
    let fields = [
        &s.state,
        &s.active,
        &s.message,
        &s.window.app_id,
        &s.window.class,
        &s.window.exe,
        &s.window.path,
        &s.window.title,
        &s.last_app.app_id,
        &s.last_app.class,
        &s.last_app.exe,
        &s.last_app.path,
        &s.last_app.title,
        &revision,
        &s.last_event,
    ];
    let _ = atomic_write(
        &runtime().join("status"),
        fields
            .iter()
            .map(|s| clean(s))
            .collect::<Vec<_>>()
            .join("\t")
            .as_bytes(),
    );
}
pub fn status() -> Status {
    let t = fs::read_to_string(runtime().join("status")).unwrap_or_default();
    let f: Vec<_> = t.split('\t').collect();
    let at = |i| f.get(i).copied().unwrap_or("").to_owned();
    Status {
        revision: at(13).parse().unwrap_or(0),
        last_event: at(14),
        state: at(0),
        active: at(1),
        message: at(2),
        window: Window {
            app_id: at(3),
            class: at(4),
            exe: at(5),
            path: at(6),
            title: at(7),
        },
        last_app: Window {
            app_id: at(8),
            class: at(9),
            exe: at(10),
            path: at(11),
            title: at(12),
        },
    }
}
pub fn autostart_path() -> PathBuf {
    preset::path()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("autostart/mchose-auto.desktop")
}
pub fn set_autostart(enabled: bool) -> io::Result<()> {
    let path = autostart_path();
    if !enabled {
        return match fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        };
    }
    let exe = std::env::current_exe()?.with_file_name("mchose");
    let exec = exe
        .to_string_lossy()
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"")
        .replace('$', "\\\\$")
        .replace('`', "\\\\`")
        .replace('%', "%%");
    atomic_write(&path,format!("[Desktop Entry]\nType=Application\nName=MCHOSE automatic presets\nExec=\"{exec}\" auto start\nIcon=mchose\nTerminal=false\n").as_bytes())
}
struct Bridge {
    child: Child,
    rx: mpsc::Receiver<Window>,
}
fn percent(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
impl Bridge {
    fn new() -> io::Result<Self> {
        prepare()?;
        let script = runtime().join("kwin-focus.py");
        fs::write(&script, include_str!("../integrations/kwin-focus.py"))?;
        let mut child = Command::new("python3")
            .arg("-u")
            .arg(script)
            .arg(runtime().join("kwin-focus.js"))
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in io::BufReader::new(stdout).lines().map_while(Result::ok) {
                if !line.starts_with("WINDOW\t") {
                    continue;
                }
                let f: Vec<_> = line.split('\t').skip(1).map(percent).collect();
                if f.len() != 4 {
                    continue;
                }
                let pid = f[3].parse::<u32>().unwrap_or(0);
                let path = fs::read_link(format!("/proc/{pid}/exe")).unwrap_or_default();
                let app = f[0]
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(".desktop")
                    .to_owned();
                let w = Window {
                    app_id: app,
                    class: f[1].clone(),
                    title: f[2].clone(),
                    exe: path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    path: path.to_string_lossy().into_owned(),
                };
                if tx.send(w).is_err() {
                    break;
                }
            }
        });
        Ok(Self { child, rx })
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        for _ in 0..15 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn device() -> io::Result<HidRaw> {
    let node = hidraw::nodes()?
        .into_iter()
        .find(|n| matches!(n.vid, 0x5253 | 0x3837) && hidraw::has_config_collection(&n.descriptor))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "MCHOSE mouse not found"))?;
    HidRaw::open(&node.dev)
}
struct Baseline {
    config: Config,
    preset: Preset,
    identity: (u16, u16, u32),
}
impl Baseline {
    fn read(dev: &HidRaw) -> io::Result<Self> {
        let id = proto::identity(dev)?;
        Ok(Self {
            config: Config::read(dev)?,
            preset: preset::current(dev)?,
            identity: (id.vid, id.pid, id.firmware),
        })
    }
    fn check(&self, dev: &HidRaw) -> io::Result<()> {
        let id = proto::identity(dev)?;
        if (id.vid, id.pid, id.firmware) != self.identity {
            return Err(invalid("mouse changed; refusing to restore another device"));
        }
        Ok(())
    }
    fn restore(&self) -> io::Result<()> {
        let dev = device()?;
        self.check(&dev)?;
        self.config.write(&dev)?;
        proto::confirm(&dev, &self.config)?;
        let mut desktop =
            preset::get("desktop").ok_or_else(|| invalid("desktop preset missing"))?;
        if desktop.rotation.is_none() {
            desktop.rotation = self.preset.rotation;
        }
        if desktop.system.is_none() {
            desktop.system = self.preset.system;
        }
        preset::apply(&dev, &desktop)
    }
}
static QUIT: AtomicBool = AtomicBool::new(false);
extern "C" fn quit(_: i32) {
    QUIT.store(true, Ordering::Relaxed);
}
/// Wait for stable focus for 400ms, then change only on matched-preset transitions.
pub fn run() -> io::Result<()> {
    let _lock = lock()?;
    let _ = fs::remove_file(runtime().join("stop"));
    QUIT.store(false, Ordering::Relaxed);
    unsafe {
        libc::signal(libc::SIGTERM, quit as *const () as usize);
        libc::signal(libc::SIGINT, quit as *const () as usize);
    }
    let notifier = crate::notifications::Notifier::new();
    let mut bridge = match Bridge::new() {
        Ok(b) => b,
        Err(e) => {
            notifier.event(
                crate::i18n::text("Automatic switching failed", "自动切换启动失败"),
                &e.to_string(),
            );
            return Err(e);
        }
    };
    let mut status = Status {
        state: "starting".into(),
        ..Status::default()
    };
    write_status(&status);
    let mut baseline: Option<Baseline> = None;
    let mut desktop_pending = true;
    let mut active: Option<(String, Preset)> = None;
    let mut candidate: Option<(String, Preset)> = None;
    let mut changed = Instant::now();
    let mut retry = Instant::now();
    let started = Instant::now();
    let mut received = false;
    let result = (|| -> io::Result<()> {
        loop {
            if QUIT.load(Ordering::Relaxed) || runtime().join("stop").exists() {
                break;
            }
            let mut new_window = false;
            while let Ok(window) = bridge.rx.try_recv() {
                status.window = window;
                received = true;
                new_window = true;
            }
            if new_window && status.window.exe != "mchose-gui" && status.window.app_id != "mchose" {
                status.last_app = status.window.clone();
            }
            if bridge.child.try_wait()?.is_some() {
                return Err(invalid(
                    "KDE focus bridge stopped; requires KDE Plasma 6, Python 3 and PyGObject",
                ));
            }
            if !received && started.elapsed() > Duration::from_secs(8) {
                return Err(invalid("KWin did not report an active window"));
            }
            let rules = rules()?;
            let selected = if received {
                selected(&rules, &status.window).map(|i| &rules[i])
            } else {
                None
            };
            let wanted = selected
                .map(|r| {
                    preset::get(&r.preset)
                        .map(|p| (r.preset.clone(), p))
                        .ok_or_else(|| invalid("rule refers to a missing preset"))
                })
                .transpose()?;
            if wanted != candidate {
                candidate = wanted;
                changed = Instant::now();
                retry = Instant::now();
            }
            if received
                && (candidate != active
                    || (candidate.is_none() && (baseline.is_some() || desktop_pending)))
                && changed.elapsed() >= Duration::from_millis(400)
                && Instant::now() >= retry
            {
                let operation = (|| -> io::Result<()> {
                    match &candidate {
                        Some((_, p)) => {
                            let dev = device()?;
                            if baseline.is_none() {
                                baseline = Some(Baseline::read(&dev)?);
                                proto::backup_original(&dev)?;
                            }
                            baseline.as_ref().unwrap().check(&dev)?;
                            // Each rule starts from the same captured settings; inherited angles never leak from another rule.
                            let mut target = *p;
                            if target.rotation.is_none() {
                                target.rotation = baseline.as_ref().unwrap().preset.rotation;
                            }
                            if target.system.is_none() {
                                target.system = baseline.as_ref().unwrap().preset.system;
                            }
                            // A system-changing rule needs a restorable baseline.
                            if target.system.is_some()
                                && baseline.as_ref().unwrap().preset.system.is_none()
                            {
                                return Err(invalid(
                                    "Cannot capture system mouse settings for restoration",
                                ));
                            }
                            preset::apply(&dev, &target)?;
                        }
                        None => {
                            if let Some(b) = &baseline {
                                b.restore()?;
                            } else {
                                // On startup outside a matched app, apply desktop too.
                                // Release the device handle before restore opens it again.
                                let current = {
                                    let dev = device()?;
                                    Baseline::read(&dev)?
                                };
                                current.restore()?;
                            }
                            baseline = None;
                        }
                    }
                    Ok(())
                })();
                match operation {
                    Ok(()) => {
                        active = candidate.clone();
                        desktop_pending = false;
                        status.message.clear();
                        status.revision = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        status.last_event = if let Some((name, p)) = &active {
                            format!(
                                "{}: {name} · {} DPI · {} Hz",
                                crate::i18n::text("Applied preset", "已应用预设"),
                                p.dpi,
                                p.rate_hz
                            )
                        } else {
                            crate::i18n::text("Applied desktop preset", "已切回 desktop 预设")
                                .into()
                        };
                        notifier.event("MCHOSE Control", &status.last_event);
                    }
                    Err(e) => {
                        active = None;
                        let message = e.to_string();
                        if message != status.message {
                            notifier.event(
                                crate::i18n::text(
                                    "Preset switch / restore failed",
                                    "预设切换或恢复失败",
                                ),
                                &message,
                            );
                        }
                        status.message = message;
                        retry = Instant::now() + Duration::from_secs(3);
                    }
                }
            }
            status.active = active.as_ref().map(|v| v.0.clone()).unwrap_or_default();
            status.state = if !status.message.is_empty() {
                "error"
            } else if !received {
                "starting"
            } else if active.is_some() {
                "matched"
            } else {
                "watching"
            }
            .into();
            write_status(&status);
            std::thread::sleep(Duration::from_millis(150));
        }
        Ok(())
    })();
    let had_baseline = baseline.is_some();
    let restored = if let Some(b) = baseline {
        b.restore()
    } else {
        Ok(())
    };
    drop(bridge);
    status.active.clear();
    status.state = "stopped".into();
    if let Err(e) = result.as_ref() {
        status.message = e.to_string();
    }
    if let Err(e) = restored.as_ref() {
        status.state = "restore-error".into();
        status.message = e.to_string();
    }
    if had_baseline && restored.is_ok() {
        status.revision = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        status.last_event = crate::i18n::text(
            "Stopped and applied desktop preset",
            "已停止并切回 desktop 预设",
        )
        .into();
        notifier.event("MCHOSE Control", &status.last_event);
    }
    if let Err(e) = result.as_ref().and(restored.as_ref()) {
        notifier.event(
            crate::i18n::text(
                "Automatic switching stopped with an error",
                "自动切换异常停止",
            ),
            &e.to_string(),
        );
    }
    write_status(&status);
    result.and(restored)
}
/// Inspect focus without starting automatic device writes.
pub fn inspect() -> io::Result<Window> {
    let bridge = Bridge::new()?;
    bridge
        .rx
        .recv_timeout(Duration::from_secs(6))
        .map_err(|_| invalid("KDE focus bridge did not report a window"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_precedence_quotes_globs_and_exclusions() {
        let w = Window {
            app_id: "steam_app_730".into(),
            exe: "cs2".into(),
            title: "Counter-Strike 2".into(),
            ..Window::default()
        };
        assert!(matches("app_id=steam_app_* && !title=*Launcher*", &w).unwrap());
        assert!(matches("exe=CS? && title=\"Counter-Strike 2\"", &w).unwrap());
        assert!(matches("app_id=other || (exe=cs2 && !class=launcher)", &w).unwrap());
        assert!(!matches("app_id=other || exe=cs2 && title=Launcher", &w).unwrap());
    }
    #[test]
    fn no_active_window_does_not_match_a_wildcard() {
        assert_eq!(
            selected(
                &[Rule {
                    enabled: true,
                    preset: "cs".into(),
                    filter: "exe=*".into()
                }],
                &Window::default()
            ),
            None
        );
    }
    #[test]
    fn rejects_bad_syntax() {
        for s in [
            "",
            "unknown=x",
            "exe=",
            "exe=x & class=y",
            "(exe=x",
            "exe=\"oops",
            "exe=x title=y",
        ] {
            assert!(validate_filter(s).is_err(), "{s}");
        }
    }
    #[test]
    fn first_enabled_match_wins() {
        let rules = vec![
            Rule {
                enabled: false,
                preset: "a".into(),
                filter: "exe=*".into(),
            },
            Rule {
                enabled: true,
                preset: "b".into(),
                filter: "exe=cs2".into(),
            },
            Rule {
                enabled: true,
                preset: "c".into(),
                filter: "exe=*".into(),
            },
        ];
        assert_eq!(
            selected(
                &rules,
                &Window {
                    exe: "cs2".into(),
                    ..Window::default()
                }
            ),
            Some(1)
        );
    }
}
