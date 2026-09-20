//! Native MCHOSE control centre. Device I/O stays on one serial worker.
use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Vec2};
use mchose::{
    hidraw::{self, HidRaw},
    i18n::{self, Language},
    preset::{self, Preset},
    proto::{self, Config, DPI_STAGES},
};
use std::{
    collections::VecDeque,
    sync::mpsc::{channel, Receiver, Sender},
    thread,
};

const BG: Color32 = Color32::from_rgb(13, 16, 24);
const CARD: Color32 = Color32::from_rgb(22, 27, 39);
const MUTED: Color32 = Color32::from_rgb(149, 162, 184);
const TEXT: Color32 = Color32::from_rgb(232, 238, 248);
const ACCENT: Color32 = Color32::from_rgb(164, 139, 250);
const TEAL: Color32 = Color32::from_rgb(94, 221, 193);
const BAD: Color32 = Color32::from_rgb(255, 149, 141);
const LINE: Color32 = Color32::from_rgb(47, 57, 76);

#[derive(Clone, Default)]
struct Snapshot {
    present: bool,
    model: String,
    firmware: String,
    link: u8,
    battery: u8,
    charging: bool,
    online: bool,
    dpi: [u16; DPI_STAGES],
    stage: u8,
    rate: u8,
    rotation: i8,
    debounce: u8,
    sleep: u8,
    lod: u8,
    ripple: bool,
    angle_snap: bool,
    motion_sync: bool,
    game_mode: u8,
}
impl Snapshot {
    fn preset(&self) -> Preset {
        Preset {
            stage: self.stage,
            dpi: self.dpi[self.stage.min(5) as usize],
            rate_hz: proto::rate_hz(self.rate).unwrap_or(0),
            lod: self.lod,
            motion_sync: self.motion_sync,
            ripple: self.ripple,
            angle_snap: self.angle_snap,
            debounce_ms: self.debounce,
            sleep_min: self.sleep,
            game_mode: self.game_mode,
            rotation: Some(self.rotation),
            system: None,
        }
    }
}
#[derive(Clone)]
enum Cmd {
    Refresh,
    System(Option<mchose::system::Settings>),
    Dpi(usize, u16),
    Stage(u8),
    Rate(u8),
    Rotation(i8),
    Debounce(u8),
    Sleep(u8),
    Flags {
        lod: u8,
        ripple: bool,
        angle_snap: bool,
        motion_sync: bool,
    },
    GameMode(u8),
    Preset(String),
    SavePreset(String),
    DeletePreset(String),
}
enum Msg {
    System(Result<(mchose::system::Settings, String), String>),
    State(Box<Snapshot>),
    Error(String),
}

fn open() -> Result<(HidRaw, String), String> {
    let node = hidraw::nodes()
        .map_err(err)?
        .into_iter()
        .find(|n| matches!(n.vid, 0x5253 | 0x3837) && hidraw::has_config_collection(&n.descriptor))
        .ok_or("No MCHOSE mouse found. Connect the mouse or receiver.")?;
    let dev = HidRaw::open(&node.dev).map_err(|e| format!("{}: {e}", node.dev.display()))?;
    Ok((dev, node.name.trim_start_matches("RealTek ").to_owned()))
}
fn err(e: std::io::Error) -> String {
    e.to_string()
}
fn edit(dev: &HidRaw, f: impl FnOnce(&mut Config)) -> Result<(), String> {
    let mut c = Config::read(dev).map_err(err)?;
    f(&mut c);
    c.write(dev).map_err(err)?;
    proto::confirm(dev, &c).map_err(err)?;
    Ok(())
}
fn read(dev: &HidRaw, model: String, firmware: String) -> Result<Snapshot, String> {
    let id = proto::identity(dev).map_err(err)?;
    let c = Config::read(dev).map_err(err)?;
    let (ripple, angle_snap, motion_sync) = c.perf_flags();
    Ok(Snapshot {
        present: true,
        model,
        firmware,
        link: id.connect_mode,
        battery: id.battery,
        charging: id.charging != 0,
        online: id.connected,
        dpi: std::array::from_fn(|i| c.dpi(i)),
        stage: if id.connect_mode == 0 {
            c.wired_dpi_stage()
        } else {
            c.wireless_dpi_stage()
        }
        .min(5),
        rate: if id.connect_mode == 0 {
            c.wired_rate_index()
        } else {
            c.wireless_rate_index()
        },
        rotation: c.rotation(),
        debounce: c.debounce_ms(),
        sleep: c.sleep_minutes(),
        lod: proto::stored_lod(),
        ripple,
        angle_snap,
        motion_sync,
        game_mode: proto::basics(dev).map_err(err)?.3,
    })
}
fn worker(rx: Receiver<Cmd>, tx: Sender<Msg>, ctx: egui::Context) {
    let mut firmware_cache: Option<(String, String)> = None;
    while let Ok(cmd) = rx.recv() {
        if let Cmd::System(settings) = cmd {
            let result = (|| {
                if let Some(s) = settings {
                    mchose::system::apply(s).map_err(err)?;
                }
                mchose::system::current().map_err(err)
            })();
            let _ = tx.send(Msg::System(result));
            ctx.request_repaint();
            continue;
        }
        let result = (|| -> Result<Snapshot, String> {
            let (dev, model) = open()?;
            if matches!(cmd, Cmd::Refresh) {
                firmware_cache = None;
            }
            let key = format!("{}:{model}", dev.path.display());
            let firmware = match &firmware_cache {
                Some((k, v)) if *k == key => v.clone(),
                _ => {
                    let v = proto::version_string(&dev).map_err(err)?;
                    firmware_cache = Some((key, v.clone()));
                    v
                }
            };
            if !matches!(
                cmd,
                Cmd::Refresh | Cmd::SavePreset(_) | Cmd::DeletePreset(_)
            ) {
                proto::backup_original(&dev).map_err(err)?;
            }
            match cmd {
                Cmd::System(_) => unreachable!(),
                Cmd::Refresh => {}
                Cmd::Dpi(i, v) => edit(&dev, |c| c.set_dpi(i, v))?,
                Cmd::Stage(v) => edit(&dev, |c| c.set_dpi_stage(v))?,
                Cmd::Debounce(v) => edit(&dev, |c| c.set_debounce_ms(v))?,
                Cmd::Sleep(v) => edit(&dev, |c| c.set_sleep_minutes(v))?,
                Cmd::Rotation(v) => proto::set_rotation(&dev, v).map_err(err)?,
                Cmd::Rate(v) => {
                    proto::set_report_rate(&dev, v, v).map_err(err)?;
                    let mut confirmed = false;
                    for _ in 0..5 {
                        thread::sleep(std::time::Duration::from_millis(80));
                        let c = Config::read(&dev).map_err(err)?;
                        if c.wired_rate_index() == v && c.wireless_rate_index() == v {
                            confirmed = true;
                            break;
                        }
                    }
                    if !confirmed {
                        return Err("Polling rate was not confirmed by the mouse".into());
                    }
                }
                Cmd::Flags {
                    lod,
                    ripple,
                    angle_snap,
                    motion_sync,
                } => proto::set_flags(&dev, lod, ripple, angle_snap, motion_sync).map_err(err)?,
                Cmd::GameMode(v) => {
                    proto::set_game_mode(&dev, v).map_err(err)?;
                    thread::sleep(std::time::Duration::from_millis(80));
                    if proto::basics(&dev).map_err(err)?.3 != v {
                        return Err("Performance mode was not confirmed".into());
                    }
                }
                Cmd::Preset(name) => {
                    preset::apply(&dev, &preset::get(&name).ok_or("Preset not found")?)
                        .map_err(err)?
                }
                Cmd::SavePreset(name) => {
                    let snapshot = read(&dev, model.clone(), firmware.clone())?;
                    let mut p = snapshot.preset();
                    p.system = mchose::system::current().ok().map(|v| v.0);
                    preset::save(&name, &p).map_err(err)?;
                }
                Cmd::DeletePreset(name) => preset::remove(&name).map_err(err)?,
            }
            read(&dev, model, firmware)
        })();
        let failed = result.is_err();
        let _ = tx.send(match result {
            Ok(s) => Msg::State(Box::new(s)),
            Err(e) => Msg::Error(e),
        });
        if failed {
            firmware_cache = None;
        }
        ctx.request_repaint();
    }
}
fn same_setting(a: &Cmd, b: &Cmd) -> bool {
    match (a, b) {
        (Cmd::Dpi(a, _), Cmd::Dpi(b, _)) => a == b,
        (Cmd::Rotation(_), Cmd::Rotation(_))
        | (Cmd::Rate(_), Cmd::Rate(_))
        | (Cmd::Debounce(_), Cmd::Debounce(_))
        | (Cmd::Sleep(_), Cmd::Sleep(_))
        | (Cmd::Flags { .. }, Cmd::Flags { .. })
        | (Cmd::GameMode(_), Cmd::GameMode(_)) => true,
        _ => false,
    }
}
fn preview(s: &mut Snapshot, cmd: &Cmd) {
    match cmd {
        Cmd::Dpi(i, v) => s.dpi[*i] = *v,
        Cmd::Stage(v) => s.stage = *v,
        Cmd::Rate(v) => s.rate = *v,
        Cmd::Rotation(v) => s.rotation = *v,
        Cmd::Debounce(v) => s.debounce = *v,
        Cmd::Sleep(v) => s.sleep = *v,
        Cmd::GameMode(v) => s.game_mode = *v,
        Cmd::Flags {
            lod,
            ripple,
            angle_snap,
            motion_sync,
        } => {
            s.lod = *lod;
            s.ripple = *ripple;
            s.angle_snap = *angle_snap;
            s.motion_sync = *motion_sync;
        }
        Cmd::Preset(name) => {
            if let Some(p) = preset::get(name) {
                s.stage = p.stage.min(5);
                s.dpi[s.stage as usize] = p.dpi;
                s.rate = proto::rate_index(p.rate_hz).unwrap_or(s.rate);
                s.rotation = p.rotation.unwrap_or(s.rotation);
                s.debounce = p.debounce_ms;
                s.sleep = p.sleep_min;
                s.lod = p.lod;
                s.ripple = p.ripple;
                s.angle_snap = p.angle_snap;
                s.motion_sync = p.motion_sync;
                s.game_mode = p.game_mode;
            }
        }
        _ => {}
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Aim,
    Sensor,
    Presets,
    Auto,
    System,
    Desktop,
}
struct App {
    tx: Sender<Cmd>,
    rx: Receiver<Msg>,
    state: Snapshot,
    confirmed: Snapshot,
    pending: VecDeque<Cmd>,
    busy: bool,
    error: Option<String>,
    language: Language,
    page: Page,
    dpi_draft: Option<u16>,
    angle_draft: Option<i8>,
    debounce_draft: Option<u8>,
    sleep_draft: Option<u8>,
    presets: Vec<(String, Preset)>,
    name: String,
    preset_editor: Option<(String, Preset)>,
    app_icons: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    delete_confirm: Option<String>,
    demo: bool,
    font_missing: bool,
    screenshot: Option<String>,
    frames: usize,
    screenshot_requested: bool,
    auto_rules: Vec<mchose::auto::Rule>,
    auto_status: mchose::auto::Status,
    auto_running: bool,
    auto_poll: std::time::Instant,
    auto_refresh_pending: bool,
    rule_preset: String,
    rule_filter: String,
    auto_error: Option<String>,
    notification_result: Option<Receiver<Result<(), String>>>,
    applications: Vec<mchose::applications::Application>,
    app_search: String,
    app_selected: Option<String>,
    system: Option<(mchose::system::Settings, String)>,
    system_error: Option<String>,
    system_speed: f64,
    tray: Option<mchose::desktop::Tray>,
    tray_ready: bool,
    window_control: Option<mchose::desktop::WindowControl>,
    close_to_tray: bool,
    background: bool,
    quitting: bool,
    allow_close: bool,
}
impl App {
    fn new(
        ctx: &egui::Context,
        language: Language,
        demo: bool,
        font_missing: bool,
        screenshot: Option<String>,
    ) -> Self {
        let capture = screenshot.is_some();
        let (tx, cmd_rx) = channel();
        let (msg_tx, rx) = channel();
        if !demo {
            let ctx = ctx.clone();
            thread::spawn(move || worker(cmd_rx, msg_tx, ctx));
            let _ = tx.send(Cmd::Refresh);
        }
        let state = if demo {
            Snapshot {
                present: true,
                model: "MCHOSE A7 Pro".into(),
                firmware: "5.x · Preview".into(),
                link: 1,
                battery: 86,
                online: true,
                dpi: [400, 800, 1600, 3200, 6400, 26000],
                stage: 1,
                rate: 2,
                rotation: -8,
                debounce: 3,
                game_mode: 3,
                ..Snapshot::default()
            }
        } else {
            Snapshot::default()
        };
        Self {
            tx,
            rx,
            confirmed: state.clone(),
            state,
            pending: if demo {
                VecDeque::new()
            } else {
                VecDeque::from([Cmd::System(None)])
            },
            busy: !demo,
            error: None,
            language,
            page: Page::Aim,
            dpi_draft: None,
            angle_draft: None,
            debounce_draft: None,
            sleep_draft: None,
            presets: preset::all().into_iter().collect(),
            name: String::new(),
            preset_editor: None,
            app_icons: Default::default(),
            delete_confirm: None,
            demo,
            font_missing,
            screenshot,
            frames: 0,
            screenshot_requested: false,
            auto_rules: if demo {
                Vec::new()
            } else {
                mchose::auto::rules().unwrap_or_default()
            },
            auto_status: if demo {
                Default::default()
            } else {
                mchose::auto::status()
            },
            auto_running: !demo && mchose::auto::running(),
            auto_poll: std::time::Instant::now(),
            auto_refresh_pending: false,
            rule_preset: "cs2".into(),
            rule_filter: "app_id=steam_app_730 || exe=cs2".into(),
            auto_error: None,
            notification_result: None,
            applications: if demo {
                vec![
                    mchose::applications::Application {
                        id: "steam_app_730".into(),
                        icon: "steam_icon_730".into(),
                        name: "Counter-Strike 2".into(),
                        chinese_name: "反恐精英 2".into(),
                        filter: "app_id=steam_app_730 || exe=cs2".into(),
                    },
                    mchose::applications::Application {
                        id: "firefox".into(),
                        icon: "firefox".into(),
                        name: "Firefox".into(),
                        chinese_name: "Firefox 浏览器".into(),
                        filter: "app_id=firefox".into(),
                    },
                ]
            } else {
                mchose::applications::installed()
            },
            app_search: String::new(),
            app_selected: None,
            system: if demo {
                Some((
                    mchose::system::Settings {
                        speed: 0.0,
                        flat: true,
                    },
                    "MCHOSE A7 Pro".into(),
                ))
            } else {
                None
            },
            system_error: None,
            system_speed: 0.0,
            tray: if demo || capture {
                None
            } else {
                mchose::desktop::Tray::start(language.resolve() == Language::Chinese).ok()
            },
            tray_ready: false,
            window_control: if demo {
                None
            } else {
                Some(mchose::desktop::WindowControl::new())
            },
            close_to_tray: mchose::desktop::close_to_tray(),
            background: false,
            quitting: false,
            allow_close: false,
        }
    }
    fn system_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;
        ui.heading(lang.text("System mouse", "系统鼠标"));
        note(
            ui,
            lang.text(
                "KDE / libinput · per-device pointer settings",
                "KDE / libinput · 独立设置这只鼠标的系统指针",
            ),
        );
        ui.add_space(16.0);
        if let Some(e) = &self.system_error {
            ui.colored_label(BAD, e);
        }
        if ui
            .button(lang.text("Read system settings", "刷新系统设置"))
            .clicked()
        {
            self.send(Cmd::System(None));
        }
        if let Some((settings, name)) = self.system.clone() {
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.label(RichText::new(name).strong().size(19.0));
                ui.add_space(12.0);
                ui.label(
                    RichText::new(if settings.flat {
                        lang.text("ACCELERATION OFF", "鼠标加速已关闭")
                    } else {
                        lang.text("ADAPTIVE ACCELERATION", "自适应加速已开启")
                    })
                    .color(if settings.flat { TEAL } else { ACCENT })
                    .size(23.0),
                );
                note(ui,lang.text("Flat keeps a constant scale. Adaptive changes the scale with movement speed.","关闭加速后使用恒定缩放；自适应加速会随移动快慢改变指针速度。"));
                ui.add_space(10.0);
                ui.add_enabled_ui(!self.busy, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(
                                settings.flat,
                                lang.text("Off · flat", "关闭加速 · 恒定"),
                            )
                            .clicked()
                        {
                            self.send(Cmd::System(Some(mchose::system::Settings {
                                flat: true,
                                ..settings
                            })));
                        }
                        if ui
                            .selectable_label(
                                !settings.flat,
                                lang.text("On · adaptive", "开启加速 · 自适应"),
                            )
                            .clicked()
                        {
                            self.send(Cmd::System(Some(mchose::system::Settings {
                                flat: false,
                                ..settings
                            })));
                        }
                    });
                });
            });
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.label(
                    RichText::new(lang.text("Pointer speed", "系统指针速度"))
                        .size(19.0)
                        .strong(),
                );
                ui.add_space(10.0);
                ui.add_enabled_ui(!self.busy, |ui| {
                    let r = ui.add(
                        egui::Slider::new(&mut self.system_speed, -100.0..=100.0).step_by(1.0),
                    );
                    if commit(&r) {
                        self.send(Cmd::System(Some(mchose::system::Settings {
                            speed: self.system_speed / 100.0,
                            ..settings
                        })));
                    }
                });
                note(ui,lang.text("−100 slow · 0 neutral · +100 fast. This is KDE's speed scale, separate from sensor DPI.","−100 慢 · 0 默认 · +100 快。这是 KDE 的速度刻度，与鼠标 DPI 分开设置。"));
            });
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.label(
                    RichText::new(lang.text("Ready for CS", "为 CS 准备"))
                        .color(TEAL)
                        .strong()
                        .size(19.0),
                );
                note(ui,lang.text("Disable acceleration + neutral speed. Saved presets include these settings; automatic switching applies desktop when focus leaves the game.","关闭加速 + 默认速度。保存预设时会包含这些设置；前台切出游戏后应用 desktop 默认预设。"));
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new(
                            lang.text("Apply recommended system settings", "应用推荐的系统设置"),
                        ),
                    )
                    .clicked()
                {
                    self.send(Cmd::System(Some(mchose::system::Settings {
                        speed: 0.0,
                        flat: true,
                    })));
                }
            });
        }
        ui.add_space(12.0);
        note(ui,lang.text("Games using raw input may bypass system pointer settings. In-game sensitivity is separate.","使用原始输入的游戏可能绕过系统指针设置，游戏内灵敏度仍需独立调整。"));
    }
    fn desktop_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;
        ui.heading(lang.text("Desktop preferences", "桌面与驻留"));
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.label(
                RichText::new(lang.text("Always within reach", "随时可用，专注游戏"))
                    .strong()
                    .size(21.0),
            );
            ui.add_space(12.0);
            ui.label(if self.tray_ready {
                lang.text("System tray connected", "系统托盘已连接")
            } else {
                lang.text(
                    "Tray unavailable · closing will keep a visible exit path",
                    "托盘不可用 · 不会将窗口隐藏到无法访问的位置",
                )
            });
            let mut close = self.close_to_tray;
            if ui
                .checkbox(
                    &mut close,
                    lang.text("Close window to tray", "关闭窗口时驻留托盘"),
                )
                .changed()
            {
                if self.demo {
                    self.close_to_tray = close;
                } else {
                    match mchose::desktop::set_close_to_tray(close) {
                        Ok(()) => self.close_to_tray = close,
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            let mut enabled = !self.demo && mchose::desktop::autostart_path().exists();
            if ui
                .checkbox(
                    &mut enabled,
                    lang.text("Start in tray when I log in", "登录后自动启动到托盘"),
                )
                .changed()
                && !self.demo
            {
                self.auto_result(mchose::desktop::set_autostart(enabled));
            }
            let mut auto = !self.demo && mchose::auto::autostart_path().exists();
            if ui
                .checkbox(
                    &mut auto,
                    lang.text(
                        "Start automatic presets when I log in",
                        "登录后启用应用自动切换",
                    ),
                )
                .changed()
                && !self.demo
            {
                self.auto_result(mchose::auto::set_autostart(auto));
            }
            note(ui,lang.text("Opening MCHOSE again shows the existing window. Quitting stops automatic presets and applies desktop.","再次打开应用会唤回已有窗口。退出程序会停止自动切换，并应用 desktop 默认预设。"));
            ui.add_space(12.0);
            if ui
                .button(lang.text("Quit and restore settings", "退出并恢复设置"))
                .clicked()
            {
                self.request_quit();
            }
            if mchose::auto::status().state == "restore-error" {
                ui.colored_label(
                    BAD,
                    lang.text(
                        "Restoration failed. Review the error before exiting.",
                        "设置恢复失败，请先检查上方错误信息。",
                    ),
                );
                if ui
                    .button(lang.text("Exit without successful restoration", "放弃恢复并退出"))
                    .clicked()
                {
                    self.allow_close = true;
                }
            }
        });
    }
    fn request_quit(&mut self) {
        if self.demo {
            self.allow_close = true;
            return;
        }
        match mchose::auto::stop() {
            Ok(()) => self.quitting = true,
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn hide_window(&self, ctx: &egui::Context) {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            if let Some(control) = &self.window_control {
                control.set_hidden(true);
            }
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }
    fn desktop_events(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let events: Vec<String> = self
            .tray
            .as_ref()
            .map(|t| t.events.try_iter().collect())
            .unwrap_or_default();
        let mut show = !self.demo && mchose::desktop::take_show();
        for event in events {
            match event.as_str() {
                "ready" => {
                    self.tray_ready = true;
                    if self.background {
                        self.hide_window(ctx);
                        self.background = false;
                    }
                }
                "unavailable" => {
                    self.tray_ready = false;
                    self.background = false;
                    show = true;
                }
                "show" => show = true,
                "start" => self.auto_result(mchose::auto::start()),
                "stop" => self.auto_result(mchose::auto::stop()),
                "quit" => self.request_quit(),
                _ => {}
            }
        }
        if self.quitting && !self.busy && !mchose::auto::running() {
            let status = mchose::auto::status();
            if status.state == "restore-error" {
                self.error = Some(status.message);
                self.quitting = false;
                show = true;
            } else {
                self.allow_close = true;
            }
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.screenshot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.close_to_tray && self.tray_ready {
                self.hide_window(ctx);
            } else {
                self.request_quit();
            }
        }
        if let Some(control) = &self.window_control {
            for error in control.errors.try_iter() {
                self.error = Some(error);
                self.tray_ready = false;
            }
        }
        if show {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                if let Some(control) = &self.window_control {
                    control.set_hidden(false);
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }
    fn flush_auto_refresh(&mut self) {
        if self.auto_refresh_pending && !self.busy {
            self.auto_refresh_pending = false;
            self.send(Cmd::Refresh);
        }
    }
    fn t<'a>(&self, en: &'a str, zh: &'a str) -> &'a str {
        self.language.text(en, zh)
    }
    fn send(&mut self, cmd: Cmd) {
        if self.demo {
            if let Cmd::System(Some(s)) = &cmd {
                self.system = Some((*s, "MCHOSE A7 Pro".into()));
                self.system_speed = s.speed * 100.0;
                return;
            }
        }
        if self.demo {
            preview(&mut self.state, &cmd);
            self.confirmed = self.state.clone();
            return;
        }
        preview(&mut self.state, &cmd);
        if self.busy {
            if self
                .pending
                .back()
                .is_some_and(|last| same_setting(last, &cmd))
            {
                self.pending.pop_back();
            }
            self.pending.push_back(cmd);
        } else {
            self.dispatch(cmd);
        }
    }
    fn dispatch(&mut self, cmd: Cmd) {
        if matches!(cmd, Cmd::Refresh | Cmd::Preset(_))
            && !self.pending.iter().any(|c| matches!(c, Cmd::System(_)))
        {
            self.pending.push_back(Cmd::System(None));
        }
        self.busy = true;
        if self.tx.send(cmd).is_err() {
            self.busy = false;
            self.pending.clear();
            self.state = self.confirmed.clone();
            self.error = Some("Device worker stopped. Reopen the application.".into());
        }
    }
    fn drain(&mut self) {
        if let Some(result) = self
            .notification_result
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.notification_result = None;
            self.auto_error = result.err();
        }
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = false;
            match msg {
                Msg::System(result) => match result {
                    Ok((settings, name)) => {
                        self.system_speed = settings.speed * 100.0;
                        self.system = Some((settings, name));
                        self.system_error = None;
                    }
                    Err(e) => {
                        self.system_error = Some(e);
                        self.system = None;
                    }
                },
                Msg::State(s) => {
                    self.confirmed = *s;
                    self.error = None;
                    self.presets = preset::all().into_iter().collect();
                }
                Msg::Error(e) => {
                    self.error = Some(e);
                    self.pending.clear();
                    self.dpi_draft = None;
                    self.angle_draft = None;
                    self.debounce_draft = None;
                    self.sleep_draft = None;
                }
            }
            self.state = self.confirmed.clone();
            for cmd in &self.pending {
                preview(&mut self.state, cmd);
            }
            if let Some(cmd) = self.pending.pop_front() {
                self.dispatch(cmd);
            }
        }
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("M").size(30.0).strong().color(ACCENT));
            ui.vertical(|ui| {
                ui.label(RichText::new("MCHOSE CONTROL").size(19.0).strong());
                ui.label(
                    RichText::new(self.t("Your mouse. Your aim.", "让鼠标适应你的瞄准习惯"))
                        .color(MUTED)
                        .size(12.0),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let old = self.language;
                egui::ComboBox::from_id_salt("language")
                    .width(105.0)
                    .selected_text(match self.language {
                        Language::Auto => "Auto / 自动",
                        Language::English => "English",
                        Language::Chinese => "简体中文",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.language, Language::Auto, "Auto / 自动");
                        ui.selectable_value(&mut self.language, Language::Chinese, "简体中文");
                        ui.selectable_value(&mut self.language, Language::English, "English");
                    });
                if old != self.language && !self.demo {
                    self.tray = None;
                    self.tray_ready = false;
                    self.tray =
                        mchose::desktop::Tray::start(self.language.resolve() == Language::Chinese)
                            .ok();
                    if let Err(e) = i18n::save(self.language) {
                        self.error = Some(e.to_string());
                    }
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new(self.t("Refresh", "刷新设备")))
                    .clicked()
                {
                    self.send(Cmd::Refresh);
                }
            });
        });
        ui.add_space(10.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(if self.state.present {
                            &self.state.model
                        } else {
                            self.t("Connect a mouse", "连接迈从鼠标")
                        })
                        .size(18.0)
                        .strong(),
                    );
                    let link = match self.state.link {
                        0 => self.t("USB wired", "USB 有线"),
                        1 => "2.4 GHz",
                        2 => self.t("Bluetooth", "蓝牙"),
                        _ => "—",
                    };
                    let status = if self.state.online {
                        self.t("Connected", "已连接")
                    } else {
                        self.t("Not connected", "未连接")
                    };
                    ui.label(
                        RichText::new(format!(
                            "{status}   ·   {link}   ·   {}",
                            self.state.firmware
                        ))
                        .color(MUTED)
                        .size(12.0),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.state.present {
                        ui.label(
                            RichText::new(format!("{}%", self.state.battery))
                                .color(TEAL)
                                .size(24.0),
                        );
                        ui.label(self.t(
                            if self.state.charging {
                                "Charging"
                            } else {
                                "Battery"
                            },
                            if self.state.charging {
                                "充电中"
                            } else {
                                "电量"
                            },
                        ));
                    }
                });
            });
        });
        ui.add_space(12.0);
    }
    fn aim(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;
        ui.columns(2,|cols|{
            card(&mut cols[0],|ui|{
                title(ui,lang.text("Sensitivity","灵敏度"),lang.text("Choose a DPI stage, then fine-tune it.","选择 DPI 档位，再精确调整数值。"));
                ui.add_space(12.0);
                let stage_width=(ui.available_width()-2.0*ui.spacing().item_spacing.x)/3.0;
                ui.horizontal_wrapped(|ui|{
                    for i in 0..6 {
                        if ui.add_sized([stage_width,34.0],egui::Button::new(self.state.dpi[i].to_string()).selected(self.state.stage as usize==i)).clicked(){self.dpi_draft=None;self.send(Cmd::Stage(i as u8));}
                    }
                });
                ui.add_space(16.0);
                let stage=self.state.stage.min(5) as usize;
                let mut value=self.dpi_draft.unwrap_or(self.state.dpi[stage]);
                ui.horizontal(|ui|{
                    ui.label(RichText::new(format!("{value}")).size(44.0).color(ACCENT).strong());ui.label("DPI");
                });
                ui.spacing_mut().slider_width=(ui.available_width()-12.0).max(120.0);
                let response=ui.add(egui::Slider::new(&mut value,50..=26000).logarithmic(true).step_by(50.0).show_value(false));
                if response.changed(){self.dpi_draft=Some(value);}
                if commit(&response){self.dpi_draft=None;self.send(Cmd::Dpi(stage,value));}
                ui.horizontal(|ui|{
                    ui.label(lang.text("Exact DPI","精确 DPI"));
                    let response=ui.add(egui::DragValue::new(&mut value).range(50..=26000).speed(50.0));
                    if response.changed(){self.dpi_draft=Some(value);}
                    if ui.button(lang.text("Apply","应用")).clicked(){value=(value/50*50).clamp(50,26000);self.dpi_draft=None;self.send(Cmd::Dpi(stage,value));}
                });
                note(ui,lang.text("DPI changes in steps of 50. Stage selection is stored on the mouse.","DPI 以 50 为步进。档位切换保存在鼠标中。"));
                ui.add_space(18.0);
                ui.label(RichText::new(lang.text("Polling rate","回报率")).strong());
                ui.horizontal_wrapped(|ui|{
                    for (i,hz) in proto::RATES.iter().enumerate(){if ui.selectable_label(self.state.rate as usize==i,format!("{hz} Hz")).clicked(){self.send(Cmd::Rate(i as u8));}}
                });
                note(ui,lang.text("Available rates depend on your mouse and receiver.","可用回报率取决于鼠标与接收器。"));
            });
            card(&mut cols[1],|ui|{
                title(ui,lang.text("Sensor rotation","传感器旋转"),lang.text("Compensate for your natural grip angle.","补偿握持偏角，让横向移动更自然。"));
                let mut angle=self.angle_draft.unwrap_or(self.state.rotation).clamp(-30,30);
                angle_diagram(ui,angle);
                ui.horizontal(|ui|{
                    ui.label(RichText::new(format!("{angle:+}°")).size(36.0).color(TEAL).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.button(lang.text("Reset to 0°","归零 0°")).clicked(){angle=0;self.angle_draft=None;self.send(Cmd::Rotation(0));}});
                });
                ui.spacing_mut().slider_width=(ui.available_width()-16.0).max(120.0);
                let response=ui.add(egui::Slider::new(&mut angle,-30..=30).step_by(1.0).show_value(false));
                if response.changed(){self.angle_draft=Some(angle);}
                if commit(&response){self.angle_draft=None;self.send(Cmd::Rotation(angle));}
                ui.horizontal(|ui|{
                    ui.label(lang.text("Exact angle","精确角度"));
                    let response=ui.add(egui::DragValue::new(&mut angle).range(-30..=30).suffix("°").speed(1.0));
                    if response.changed(){self.angle_draft=Some(angle);}
                    if ui.button(lang.text("Apply","应用")).clicked(){self.angle_draft=None;self.send(Cmd::Rotation(angle));}
                });
                note(ui,lang.text("−30° to +30° · 1° steps · writes on release", "−30°～+30° · 1° 步进 · 松手后写入"));
                ui.add_space(10.0);
                note(ui,lang.text("Rotation changes the sensor axes. Angle snapping straightens motion; these are different settings. Test small changes in your practice map.","旋转角度用于调整传感器坐标轴；直线修正会拉直轨迹，两者不同。建议在练习地图里逐度调整。"));
            });
        });
    }
    fn sensor(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;
        ui.columns(2,|cols|{
            card(&mut cols[0],|ui|{
                title(ui,lang.text("Tracking","传感器追踪"),lang.text("Control how motion is processed.","调整鼠标运动数据的处理方式。"));
                let (mut motion,mut ripple,mut snap,mut lod)=(self.state.motion_sync,self.state.ripple,self.state.angle_snap,self.state.lod);
                ui.add_space(14.0);
                toggle(ui,lang.text("Motion Sync","运动同步"),lang.text("Align sensor sampling with USB reports.","同步传感器采样和 USB 回报。"),&mut motion);
                toggle(ui,lang.text("Ripple control","波纹控制"),lang.text("Smooth tracking data.","对追踪数据进行平滑处理。"),&mut ripple);
                toggle(ui,lang.text("Angle snapping","直线修正"),lang.text("Straightens trajectories. Independent of grip rotation.","拉直移动轨迹，与握持角度补偿不同。"),&mut snap);
                ui.separator();
                ui.label(RichText::new(lang.text("Lift-off distance","抬升高度 LOD")).strong());
                ui.horizontal(|ui|{ui.selectable_value(&mut lod,0,"1 mm");ui.selectable_value(&mut lod,1,"2 mm");});
                note(ui,lang.text("Cached setting: hardware cannot report LOD. This mapping is for the original supported protocol.","显示工具上次写入的值，设备无法读回 LOD。此映射沿用当前支持的协议。"));
                if (motion,ripple,snap,lod)!=(self.state.motion_sync,self.state.ripple,self.state.angle_snap,self.state.lod){self.send(Cmd::Flags{lod,ripple,angle_snap:snap,motion_sync:motion});}
            });
            card(&mut cols[1],|ui|{
                title(ui,lang.text("Response & power","响应与电源"),lang.text("Tune click response and idle behaviour.","调整按键响应和空闲时的电源行为。"));
                ui.add_space(14.0);
                let mut debounce=self.debounce_draft.unwrap_or(self.state.debounce);
                ui.label(lang.text("Debounce time","按键消抖"));
                let response=ui.add(egui::Slider::new(&mut debounce,0..=30).suffix(" ms"));
                if response.changed(){self.debounce_draft=Some(debounce);}
                if commit(&response){self.debounce_draft=None;self.send(Cmd::Debounce(debounce));}
                note(ui,lang.text("Lower values may increase unintended double-clicks.","更低的数值可能增加误双击。"));
                ui.add_space(20.0);
                let mut sleep=self.sleep_draft.unwrap_or(self.state.sleep);
                ui.label(lang.text("Sleep after","休眠时间"));
                let response=ui.add(egui::Slider::new(&mut sleep,0..=60).suffix(lang.text(" min"," 分钟")));
                if response.changed(){self.sleep_draft=Some(sleep);}
                if commit(&response){self.sleep_draft=None;self.send(Cmd::Sleep(sleep));}
                note(ui,lang.text("0 disables sleep.","0 表示禁用休眠。"));
                ui.add_space(20.0);
                ui.label(RichText::new(lang.text("Performance mode","性能模式")).strong());
                ui.horizontal(|ui|{for (v,en,zh) in [(1,"Mode 1","模式 1"),(2,"Mode 2","模式 2"),(3,"Mode 3","模式 3")] {if ui.selectable_label(self.state.game_mode==v,lang.text(en,zh)).clicked(){self.send(Cmd::GameMode(v));}}});
                note(ui,lang.text("Mode meanings depend on mouse firmware.","各模式的具体含义取决于鼠标固件。"));
            });
        });
    }
    fn presets_page(&mut self, ui: &mut egui::Ui) {
        self.edit_preset_window(ui.ctx());
        let lang = self.language;
        card(ui, |ui| {
            title(
                ui,
                lang.text("Save your setup", "保存你的配置"),
                lang.text(
                    "DPI, polling, sensor settings and rotation in one preset.",
                    "一起保存 DPI、回报率、传感器参数、旋转角度与可读取的系统鼠标设置。",
                ),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .hint_text(lang.text("e.g. My CS · −8°", "例如：我的 CS · −8°"))
                        .desired_width(280.0),
                );
                let valid = preset::validate_name(&self.name).is_ok();
                if ui
                    .add_enabled(
                        valid && !self.busy,
                        egui::Button::new(lang.text("Save current settings", "保存当前设置")),
                    )
                    .clicked()
                {
                    self.send(Cmd::SavePreset(self.name.trim().to_owned()));
                    self.name.clear();
                }
            });
            note(
                ui,
                lang.text(
                    "Saving an existing name replaces that preset. System speed and acceleration are included when available.",
                    "使用已有名称保存会覆盖预设；系统速度和加速度在可读取时一并保存。",
                ),
            );
        });
        ui.add_space(12.0);
        for (name, p) in self.presets.clone() {
            card(ui, |ui| {
                let mut current = self.confirmed.preset();
                current.system = self.system.as_ref().map(|v| v.0);
                let active = preset::matches(&p, &current);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&name).size(20.0).strong());
                    if self.auto_running && self.auto_status.active == name {
                        ui.label(
                            RichText::new(lang.text("Applied by app rule", "由应用规则启用"))
                                .color(TEAL),
                        );
                    }
                    if active {
                        ui.label(
                            RichText::new(lang.text("Settings match", "参数相同")).color(TEAL),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(lang.text("Edit", "编辑")).clicked() {
                            self.preset_editor = Some((name.clone(), p));
                        }
                        if ui
                            .add_enabled(!self.busy, egui::Button::new(lang.text("Apply", "应用")))
                            .clicked()
                        {
                            self.send(Cmd::Preset(name.clone()));
                        }
                        if ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new(lang.text("Delete saved", "删除自定义")),
                            )
                            .clicked()
                        {
                            self.delete_confirm = Some(name.clone());
                        }
                    });
                });
                let angle = p
                    .rotation
                    .map(|v| format!("{v:+}°"))
                    .unwrap_or_else(|| lang.text("keep current angle", "保留当前角度").into());
                note(
                    ui,
                    &format!(
                        "{} DPI   ·   {} Hz   ·   {} ms   ·   {}",
                        p.dpi, p.rate_hz, p.debounce_ms, angle
                    ),
                );
                if let Some(settings) = p.system {
                    note(
                        ui,
                        &format!(
                            "{}: {:.0} · {}",
                            lang.text("System speed", "系统速度"),
                            settings.speed * 100.0,
                            if settings.flat {
                                lang.text("acceleration off", "加速度关闭")
                            } else {
                                lang.text("adaptive acceleration", "自适应加速度")
                            }
                        ),
                    );
                }
                if name == "desktop" {
                    note(
                        ui,
                        lang.text(
                            "Default after leaving a matched app or stopping monitoring",
                            "切出匹配应用或停止监听后使用的默认预设",
                        ),
                    );
                }
                for rule in self
                    .auto_rules
                    .clone()
                    .into_iter()
                    .filter(|r| r.preset == name)
                {
                    self.binding_preview(ui, &rule);
                }
                if self.delete_confirm.as_ref() == Some(&name) {
                    ui.horizontal(|ui| {
                        ui.label(lang.text("Delete this saved preset?", "删除此自定义预设？"));
                        if ui.button(lang.text("Delete", "确认删除")).clicked() {
                            self.send(Cmd::DeletePreset(name.clone()));
                            self.delete_confirm = None;
                        }
                        if ui.button(lang.text("Cancel", "取消")).clicked() {
                            self.delete_confirm = None;
                        }
                    });
                }
            });
            ui.add_space(10.0);
        }
        note(ui,lang.text("Edit cs2 and desktop to suit your mouse. Saving edits replaces their defaults; deleting saved edits restores factory preset values.","cs2 与 desktop 都可编辑，保存后覆盖内置值；删除自定义配置会恢复内置值。"));
    }
}
impl App {
    fn auto_result(&mut self, result: std::io::Result<()>) {
        self.auto_error = result.err().map(|e| e.to_string());
    }
    fn auto_page(&mut self, ui: &mut egui::Ui) {
        use mchose::auto;
        let lang = self.language;
        card(ui, |ui| {
            title(
                ui,
                lang.text("Follow the foreground app", "跟随前台应用"),
                lang.text(
                    "Match an app, apply its preset. Switch away to apply desktop.",
                    "前台应用命中规则时应用预设；切出后切回 desktop 默认预设。",
                ),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let mut enabled = self.auto_running;
                if ui
                    .add_enabled(
                        !self.demo,
                        egui::Checkbox::new(
                            &mut enabled,
                            lang.text("Automatic switching", "启用自动切换"),
                        ),
                    )
                    .changed()
                {
                    self.auto_result(if enabled { auto::start() } else { auto::stop() });
                }
            });
            if ui
                .link(lang.text(
                    "Configure login startup in Preferences",
                    "登录自启请到“桌面与驻留”设置",
                ))
                .clicked()
            {
                self.page = Page::Desktop;
            }
            let state = match self.auto_status.state.as_str() {
                "matched" => lang.text("Preset active", "已匹配预设"),
                "watching" => lang.text("Watching foreground app", "监听前台应用中"),
                "starting" => lang.text("Starting…", "正在启动…"),
                "restore-error" => lang.text("Restore failed", "恢复配置失败"),
                "error" => lang.text("Switch failed", "切换失败"),
                _ => lang.text("Stopped", "已停止"),
            };
            ui.label(format!("{state}   {}", self.auto_status.active));
            if !self.auto_status.last_event.is_empty() {
                ui.colored_label(TEAL, localized_event(&self.auto_status.last_event, lang));
            }
            note(
                ui,
                lang.text(
                    "Default after switching away: desktop. Edit it in Presets.",
                    "切出后的默认预设：desktop。可在预设管理中编辑。",
                ),
            );
            let mut notifications = mchose::notifications::enabled();
            if ui
                .checkbox(
                    &mut notifications,
                    lang.text(
                        "Desktop notifications for switching and errors",
                        "切换、恢复及失败时显示桌面通知",
                    ),
                )
                .changed()
                && !self.demo
            {
                self.auto_result(mchose::notifications::set_enabled(notifications));
            }
            if ui
                .button(lang.text("Test notification", "测试桌面通知"))
                .clicked()
                && !self.demo
            {
                let (tx, rx) = std::sync::mpsc::channel();
                self.notification_result = Some(rx);
                std::thread::spawn(move || {
                    let result = mchose::notifications::send(
                        "MCHOSE Control",
                        mchose::i18n::preference()
                            .text("Desktop notifications are working", "桌面通知测试成功"),
                    )
                    .map_err(|e| e.to_string());
                    let _ = tx.send(result);
                });
            }
            egui::CollapsingHeader::new(
                lang.text("Recent switch events (UTC)", "最近切换记录（UTC 时间）"),
            )
            .show(ui, |ui| {
                for line in mchose::notifications::history().lines().rev().take(8) {
                    ui.label(line);
                }
            });
            if !self.auto_status.message.is_empty() {
                ui.colored_label(
                    BAD,
                    i18n::error_text(
                        &self.auto_status.message,
                        lang.resolve() == Language::Chinese,
                    ),
                );
            }
            if let Some(e) = &self.auto_error {
                ui.colored_label(
                    BAD,
                    i18n::error_text(e, lang.resolve() == Language::Chinese),
                );
            }
            note(ui,lang.text("Monitoring continues after this window closes. Turn it off here to stop and apply desktop.","关闭此窗口后仍会继续监听；在这里关闭自动切换，即可停止并应用 desktop 默认预设。"));
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            title(
                ui,
                lang.text("Bind an application", "选择应用并绑定预设"),
                lang.text(
                    "Choose an installed app. Matching details are filled in automatically.",
                    "从已安装的应用中选择，匹配规则会自动填写。",
                ),
            );
            ui.add_space(10.0);
            ui.columns(2, |cols| {
                cols[0].add(egui::TextEdit::singleline(&mut self.app_search).hint_text(lang.text("Search applications…", "搜索应用名称…")).desired_width(f32::INFINITY));
                let query = self.app_search.to_lowercase();
                let choices: Vec<_> = self.applications.iter().filter(|a| format!("{} {} {}", a.name, a.chinese_name, a.id).to_lowercase().contains(&query)).cloned().collect();
                egui::ScrollArea::vertical().id_salt("application-picker").max_height(210.0).show(&mut cols[0], |ui| {
                    for app in &choices {
                        ui.horizontal(|ui| {
                            self.application_icon(ui, &app.icon);
                            if ui.selectable_label(self.app_selected.as_deref() == Some(&app.id), app.label(lang)).clicked() {
                                self.app_selected = Some(app.id.clone()); self.rule_filter = app.filter.clone();
                            }
                        });
                    }
                    if choices.is_empty() { note(ui,lang.text("No applications found.", "未找到应用。")); }
                });
                if cols[0].button(lang.text("Refresh application list", "刷新应用列表")).clicked() { self.applications = mchose::applications::installed(); }
                let ui = &mut cols[1];
                let selected = self.applications.iter().find(|a|Some(&a.id)==self.app_selected.as_ref()).map(|a|a.label(lang).to_owned());
                ui.label(RichText::new(selected.unwrap_or_else(||if self.app_selected.as_deref()==Some("recent"){lang.text("Recent foreground application", "最近的前台应用").into()}else{lang.text("Select an app on the left", "先选择左侧应用").into()})).size(19.0).strong());
                ui.add_space(12.0);
                ui.label(lang.text("Use preset", "使用预设"));
                egui::ComboBox::from_id_salt("rule-preset").selected_text(&self.rule_preset).width(180.0).show_ui(ui, |ui| { for (name, _) in &self.presets { ui.selectable_value(&mut self.rule_preset, name.clone(), name); } });
                ui.add_space(12.0);
                let valid = auto::validate_filter(&self.rule_filter).is_ok();
                if ui.add_enabled(self.app_selected.is_some() && valid && !self.demo,egui::Button::new(lang.text("Save binding", "保存绑定"))).clicked() {
                    let mut rules=self.auto_rules.clone(); if let Some(rule) = rules.iter_mut().find(|r| r.filter == self.rule_filter) { rule.preset=self.rule_preset.clone(); rule.enabled=true; } else { rules.push(auto::Rule{enabled:true,preset:self.rule_preset.clone(),filter:self.rule_filter.clone()}); }
                    match auto::save_rules(&rules){Ok(())=>{self.auto_rules=rules;self.auto_error=None;},Err(e)=>self.auto_error=Some(e.to_string())}
                }
                note(ui,lang.text("Switching away applies the desktop preset.", "切出匹配应用后，应用 desktop 默认预设。"));
                ui.add_space(14.0);
                if let Some(app)=mchose::applications::observed(&self.auto_status.last_app) {
                    note(ui, &format!("{} {}",lang.text("Recently focused:", "最近使用："),app.name));
                    if ui.button(lang.text("Choose this application", "选择此应用")).clicked(){self.rule_filter=app.filter;self.app_selected=Some("recent".into());}
                }
                note(ui,lang.text("App missing? Enable monitoring, focus it, then return here to choose it from recent apps.", "找不到应用？启用监听，切到该应用再回来，即可从最近使用中选择。"));
            });
            ui.add_space(10.0);
            egui::CollapsingHeader::new(lang.text("Advanced: matching rules and identifiers", "高级设置：过滤规则与应用标识")).show(ui, |ui| {
                let changed=ui.add(egui::TextEdit::singleline(&mut self.rule_filter).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace)).changed();
                if changed {self.app_selected=Some("advanced".into());}
                if let Err(e)=auto::validate_filter(&self.rule_filter){ui.colored_label(BAD,i18n::error_text(&e.to_string(),lang.resolve()==Language::Chinese));}
                let matched=auto::matches(&self.rule_filter,&self.auto_status.last_app).unwrap_or(false);
                note(ui,if matched{lang.text("Matches the last external app.","匹配最近的外部应用。")}else{lang.text("Does not match the last external app.","不匹配最近的外部应用。")});
                note(ui,lang.text("Fields: app_id, class, exe, path, title. Globs: * ?. Logic: && || ! ( ). Quote values with spaces.", "字段：app_id、class、exe、path、title。通配符：* ?；逻辑：&& || ! ( )。含空格的值使用双引号。"));
                let w=&self.auto_status.last_app;ui.monospace(format!("app_id={}\nclass={}\nexe={}\ntitle={}",w.app_id,w.class,w.exe,w.title));
            });
        });
        ui.add_space(12.0);
        let mut changed = false;
        for i in 0..self.auto_rules.len() {
            let r = self.auto_rules[i].clone();
            let app_label = self
                .applications
                .iter()
                .find(|a| a.filter == r.filter)
                .map(|a| a.label(lang).to_owned())
                .unwrap_or_else(|| lang.text("Custom application", "自定义应用").into());
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut enabled = r.enabled;
                    if ui
                        .checkbox(
                            &mut enabled,
                            format!("{} · {} → {}", i + 1, app_label, r.preset),
                        )
                        .changed()
                    {
                        self.auto_rules[i].enabled = enabled;
                        changed = true;
                    }
                    if i > 0 && ui.button(lang.text("Move up", "上移")).clicked() {
                        self.auto_rules.swap(i, i - 1);
                        changed = true;
                    }
                    if ui.button(lang.text("Remove", "移除")).clicked() {
                        self.auto_rules[i].filter.clear();
                        changed = true;
                    }
                });
                self.binding_preview(ui, &r);
                let mut selected = r.preset.clone();
                egui::ComboBox::from_id_salt(("bound-preset", i))
                    .selected_text(&selected)
                    .show_ui(ui, |ui| {
                        for (name, _) in &self.presets {
                            ui.selectable_value(&mut selected, name.clone(), name);
                        }
                    });
                if selected != r.preset {
                    self.auto_rules[i].preset = selected;
                    changed = true;
                }
                egui::CollapsingHeader::new(lang.text("Matching details", "高级匹配详情"))
                    .id_salt(i)
                    .show(ui, |ui| {
                        ui.monospace(&r.filter);
                    });
            });
            ui.add_space(8.0);
        }
        if changed && !self.demo {
            self.auto_rules.retain(|r| !r.filter.is_empty());
            self.auto_result(auto::save_rules(&self.auto_rules));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain();
        self.desktop_events(ui.ctx());
        if !self.demo && self.auto_poll.elapsed() > std::time::Duration::from_millis(500) {
            let previous = (self.auto_status.revision, self.auto_status.active.clone());
            self.auto_status = mchose::auto::status();
            self.auto_running = mchose::auto::running();
            if let Ok(rules) = mchose::auto::rules() {
                self.auto_rules = rules;
            }
            if previous != (self.auto_status.revision, self.auto_status.active.clone()) {
                self.auto_refresh_pending = true;
            }
            self.auto_poll = std::time::Instant::now();
        }
        self.flush_auto_refresh();
        if self.page == Page::Auto {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
        self.frames += 1;
        if let Some(path) = self.screenshot.clone() {
            if self.frames >= 5 && !self.busy && !self.screenshot_requested {
                self.screenshot_requested = true;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let image = ui.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = image {
                use eframe::icon_data::IconDataExt;
                let icon = egui::IconData {
                    width: image.size[0] as u32,
                    height: image.size[1] as u32,
                    rgba: image.pixels.iter().flat_map(|p| p.to_array()).collect(),
                };
                match icon
                    .to_png_bytes()
                    .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
                {
                    Ok(()) => {}
                    Err(e) => eprintln!("screenshot: {e}"),
                };
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
        Frame::new()
            .fill(BG)
            .inner_margin(Margin::same(24))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
                self.header(ui);
                if self.demo {
                    ui.label(
                        RichText::new(
                            self.t("PREVIEW · no device changes", "预览模式 · 不会更改设备"),
                        )
                        .color(ACCENT),
                    );
                }
                if self.font_missing {
                    ui.colored_label(
                        BAD,
                        "Chinese font missing. Install Noto Sans CJK to display Chinese.",
                    );
                }
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.spinner();
                    }
                    let status = if self.error.is_some() {
                        self.t(
                            "Operation failed. Refresh to read the current device state.",
                            "操作失败，请刷新以读取设备当前状态。",
                        )
                    } else if self.busy {
                        self.t("Applying / reading settings…", "正在写入或读取设置…")
                    } else {
                        self.t("Ready · settings read from mouse", "就绪 · 已读取鼠标设置")
                    };
                    ui.label(
                        RichText::new(status)
                            .color(if self.error.is_some() { BAD } else { MUTED })
                            .size(12.0),
                    );
                });
                if self.quitting {ui.label(self.t("Restoring settings before exit…", "正在恢复设置，完成后退出…"));}
                if let Some(e) = &self.error {
                    ui.label(
                        RichText::new(i18n::error_text(
                            e,
                            self.language.resolve() == Language::Chinese,
                        ))
                        .color(BAD)
                        .size(12.0),
                    );
                }
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(165.0,ui.available_height()),egui::Layout::top_down(egui::Align::Min),|ui|{
                    ui.set_width(165.0);
                    for (page,en,zh) in [(Page::Aim,"Aim & rotation","瞄准与角度"),(Page::Sensor,"Sensor","传感器与响应"),(Page::System,"System mouse","系统鼠标"),(Page::Presets,"Presets","预设管理"),(Page::Auto,"Applications","应用自动切换"),(Page::Desktop,"Preferences","桌面与驻留")] {
                        ui.add_space(6.0);
                        if ui.add_sized([160.0,44.0],egui::Button::new(RichText::new(self.t(en,zh)).size(15.0)).selected(self.page==page)).clicked(){self.page=page;if page==Page::System && !self.demo{self.send(Cmd::System(None));}}
                    }
                });
                ui.separator();
                ui.vertical(|ui|{
                ui.set_min_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if !self.state.present && !matches!(self.page,Page::System|Page::Desktop|Page::Auto) {
                            ui.add_space(45.0);
                            ui.label(self.t(
                                "Connect a MCHOSE mouse or receiver and press Refresh.",
                                "连接迈从鼠标或接收器，然后点击“刷新设备”。",
                            ));
                            return;
                        }
                        let controlled = self.auto_running && !self.auto_status.active.is_empty() && !matches!(self.page,Page::Auto|Page::Desktop);
                        if controlled {
                            note(ui, self.t("An automatic preset is active. Pause switching to edit manually.", "自动预设正在生效，暂停自动切换后可手动编辑。"));
                            if ui.button(self.t("Pause automatic switching", "暂停自动切换")).clicked() { self.auto_result(mchose::auto::stop()); }
                        }
                        ui.add_enabled_ui(!controlled, |ui| {
                        match self.page {
                            Page::Aim => self.aim(ui),
                            Page::Sensor => self.sensor(ui),
                            Page::Presets => self.presets_page(ui),
                            Page::Auto => self.auto_page(ui),
                            Page::System => self.system_page(ui),
                            Page::Desktop => self.desktop_page(ui),
                        }
                        });
                    });
                });
                });
            });
    }
}
fn commit(r: &egui::Response) -> bool {
    r.drag_stopped() || (r.changed() && !r.dragged())
}
fn card<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            f(ui)
        })
        .inner
}
fn title(ui: &mut egui::Ui, heading: &str, description: &str) {
    ui.label(RichText::new(heading).size(19.0).strong());
    note(ui, description);
}
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(MUTED).size(12.0)).wrap());
}
fn toggle(ui: &mut egui::Ui, title: &str, description: &str, value: &mut bool) {
    ui.checkbox(value, RichText::new(title).strong());
    note(ui, description);
    ui.add_space(14.0);
}
fn angle_diagram(ui: &mut egui::Ui, angle: i8) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 152.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let center = rect.center();
    p.circle_stroke(center, 63.0, Stroke::new(1.0, LINE));
    p.circle_stroke(center, 42.0, Stroke::new(1.0, LINE));
    p.line_segment(
        [
            center + Vec2::new(-90.0, 0.0),
            center + Vec2::new(90.0, 0.0),
        ],
        Stroke::new(1.0, LINE),
    );
    p.line_segment(
        [
            center + Vec2::new(0.0, -76.0),
            center + Vec2::new(0.0, 76.0),
        ],
        Stroke::new(1.0, MUTED),
    );
    let theta = (angle as f32).to_radians();
    let axis = Vec2::new(theta.sin(), -theta.cos());
    p.arrow(center - axis * 49.0, axis * 108.0, Stroke::new(3.0, TEAL));
    let perpendicular = Vec2::new(-axis.y, axis.x);
    p.line_segment(
        [center - perpendicular * 45.0, center + perpendicular * 45.0],
        Stroke::new(2.0, ACCENT),
    );
    p.circle_filled(center, 5.0, TEAL);
    p.text(
        rect.left_top() + Vec2::new(3.0, 10.0),
        egui::Align2::LEFT_TOP,
        "−30°",
        egui::FontId::proportional(12.0),
        MUTED,
    );
    p.text(
        rect.right_top() + Vec2::new(-3.0, 10.0),
        egui::Align2::RIGHT_TOP,
        "+30°",
        egui::FontId::proportional(12.0),
        MUTED,
    );
}
fn install_fonts(ctx: &egui::Context) -> bool {
    let found = std::process::Command::new("fc-match")
        .args(["-f", "%{file}\\n%{index}\\n", ":lang=zh-cn"])
        .output()
        .ok();
    let mut candidates = Vec::new();
    if let Some(output) = found {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            let mut lines = s.lines();
            if let Some(path) = lines.next() {
                candidates.push((
                    path.to_owned(),
                    lines
                        .next()
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(0),
                ));
            }
        }
    }
    candidates.push((
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".into(),
        2,
    ));
    candidates.push((
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into(),
        2,
    ));
    for (path, index) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            let mut data = egui::FontData::from_owned(bytes);
            data.index = index;
            fonts.font_data.insert("cjk".into(), data.into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
            ctx.set_fonts(fonts);
            return true;
        }
    }
    false
}
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|v| v == "--help" || v == "-h") {
        println!("MCHOSE Control Linux 0.3.3\n  --lang auto|en|zh   Language / 语言\n  --demo              Preview without device changes / 预览模式\n  --background        Start in tray / 启动到托盘\n  --page aim|sensor|system|presets|auto|desktop\n  --demo --screenshot /absolute/path.png");
        return Ok(());
    }

    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let lang = value("--lang")
        .and_then(|v| Language::parse(&v))
        .unwrap_or_else(i18n::preference);
    let demo = args.iter().any(|v| v == "--demo");
    let _instance = if demo || value("--screenshot").is_some() {
        None
    } else {
        match mchose::desktop::Instance::acquire() {
            Ok(Some(i)) => Some(i),
            Ok(None) => return Ok(()),
            Err(e) => {
                eprintln!("{e}");
                return Ok(());
            }
        }
    };
    let background = args.iter().any(|v| v == "--background");
    let screenshot = value("--screenshot");
    let app_search = value("--app-search").unwrap_or_default();
    let page = value("--page");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1010.0, 850.0])
            .with_min_inner_size([860.0, 620.0])
            .with_title("MCHOSE Mouse")
            .with_app_id("mchose")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../assets/brand/icon.png"))
                    .expect("bundled icon"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "mchose",
        options,
        Box::new(move |cc| {
            let font_missing = !install_fonts(&cc.egui_ctx);
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BG;
            visuals.window_fill = BG;
            visuals.override_text_color = Some(TEXT);
            visuals.selection.bg_fill = Color32::from_rgb(77, 62, 119);
            visuals.widgets.active.bg_fill = Color32::from_rgb(77, 62, 119);
            visuals.widgets.hovered.weak_bg_fill = LINE;
            visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(31, 38, 53);
            cc.egui_ctx.set_visuals(visuals);
            cc.egui_ctx.all_styles_mut(|s| {
                s.spacing.interact_size.y = 32.0;
                s.spacing.slider_width = 235.0;
                s.spacing.button_padding = Vec2::new(12.0, 7.0);
                s.spacing.slider_rail_height = 6.0;
            });
            let mut app = App::new(&cc.egui_ctx, lang, demo, font_missing, screenshot);
            app.background = background && !demo && app.screenshot.is_none();
            app.app_search = app_search;
            app.page = match page.as_deref() {
                Some("sensor") => Page::Sensor,
                Some("presets") => Page::Presets,
                Some("auto") => Page::Auto,
                Some("system") => Page::System,
                Some("desktop") => Page::Desktop,
                _ => Page::Aim,
            };
            Ok(Box::new(app))
        }),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> (App, Receiver<Cmd>, Sender<Msg>) {
        let (tx, commands) = channel();
        let (messages, rx) = channel();
        let mut a = App::new(
            &egui::Context::default(),
            Language::English,
            true,
            false,
            None,
        );
        a.demo = false;
        a.tx = tx;
        a.rx = rx;
        a.state.dpi = [800; 6];
        a.confirmed = a.state.clone();
        (a, commands, messages)
    }
    #[test]
    fn automatic_refresh_is_not_lost_while_device_worker_is_busy() {
        let (mut a, commands, _messages) = app();
        a.busy = true;
        a.auto_refresh_pending = true;
        a.flush_auto_refresh();
        assert!(a.auto_refresh_pending);
        assert!(commands.try_recv().is_err());
        a.busy = false;
        a.flush_auto_refresh();
        assert!(!a.auto_refresh_pending);
        assert!(matches!(commands.try_recv().unwrap(), Cmd::Refresh));
    }
    #[test]
    fn unrelated_edits_survive_and_pending_values_stay_visible() {
        let (mut a, c, m) = app();
        a.send(Cmd::Rotation(-8));
        a.send(Cmd::Debounce(4));
        a.send(Cmd::Sleep(10));
        assert!(matches!(c.recv().unwrap(), Cmd::Rotation(-8)));
        let mut s = a.confirmed.clone();
        s.rotation = -8;
        m.send(Msg::State(Box::new(s))).unwrap();
        a.drain();
        assert!(matches!(c.recv().unwrap(), Cmd::Debounce(4)));
        assert_eq!(a.state.sleep, 10);
        assert_eq!(a.state.rotation, -8);
    }
    #[test]
    fn only_adjacent_edits_are_merged() {
        let (mut a, _, _) = app();
        a.busy = true;
        a.send(Cmd::Rotation(-2));
        a.send(Cmd::Rotation(-8));
        a.send(Cmd::Preset("cs".into()));
        a.send(Cmd::Rotation(3));
        assert_eq!(a.pending.len(), 3);
        assert!(matches!(a.pending.front(), Some(Cmd::Rotation(-8))));
    }
    #[test]
    fn failed_write_clears_queue_and_rolls_back() {
        let (mut a, _c, m) = app();
        let old = a.state.rotation;
        a.send(Cmd::Rotation(5));
        a.send(Cmd::Sleep(20));
        m.send(Msg::Error("disconnected".into())).unwrap();
        a.drain();
        assert_eq!(a.state.rotation, old);
        assert!(a.pending.is_empty());
        assert!(a.error.is_some());
    }
}
impl App {
    fn application_icon(&mut self, ui: &mut egui::Ui, name: &str) {
        let texture = self.app_icons.entry(name.to_owned()).or_insert_with(|| {
            let bytes = std::fs::read(mchose::applications::icon_path(name)?).ok()?;
            let icon = eframe::icon_data::from_png_bytes(&bytes).ok()?;
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            );
            Some(
                ui.ctx()
                    .load_texture(name, image, egui::TextureOptions::LINEAR),
            )
        });
        if let Some(texture) = texture {
            ui.image((texture.id(), egui::vec2(28.0, 28.0)));
        } else {
            ui.label(RichText::new("▣").size(24.0).color(MUTED));
        }
    }
    fn binding_preview(&mut self, ui: &mut egui::Ui, rule: &mchose::auto::Rule) {
        let lang = self.language;
        let app = self
            .applications
            .iter()
            .find(|a| a.filter == rule.filter)
            .cloned();
        ui.horizontal(|ui| {
            if let Some(app) = &app {
                self.application_icon(ui, &app.icon);
                ui.label(app.label(lang));
            } else {
                self.application_icon(ui, "");
                ui.label(lang.text("Custom application rule", "自定义应用规则"));
            }
            ui.label(if rule.enabled {
                lang.text("Enabled", "已启用")
            } else {
                lang.text("Disabled", "已停用")
            });
            if rule.enabled
                && mchose::auto::matches(&rule.filter, &self.auto_status.last_app).unwrap_or(false)
            {
                ui.colored_label(TEAL, lang.text("Matches recent app", "匹配最近应用"));
            }
        });
        ui.add(
            egui::Label::new(RichText::new(&rule.filter).small().monospace().color(MUTED)).wrap(),
        );
    }
    fn edit_preset_window(&mut self, ctx: &egui::Context) {
        let Some((name, mut p)) = self.preset_editor.clone() else {
            return;
        };
        let lang = self.language;
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(format!("{} · {name}", lang.text("Edit preset", "编辑预设")))
            .id(egui::Id::new("preset-editor")).open(&mut open).default_width(520.0).resizable(true).vscroll(true)
            .show(ctx, |ui| {
                note(ui, lang.text("Save changes without applying them to the mouse. Active automatic rules pick up saved changes.", "保存不会直接写入鼠标；若此预设正由自动规则使用，监听器会应用保存后的修改。"));
                egui::Grid::new("preset-values").num_columns(2).spacing([24.0, 10.0]).show(ui, |ui| {
                    ui.label(lang.text("DPI stage", "DPI 档位"));
                    let mut stage = p.stage + 1;
                    ui.add(egui::DragValue::new(&mut stage).range(1..=6)); p.stage=stage-1; ui.end_row();
                    ui.label("DPI"); ui.add(egui::DragValue::new(&mut p.dpi).range(50..=26000).speed(50)); ui.end_row();
                    ui.label(lang.text("Polling rate", "回报率"));
                    egui::ComboBox::from_id_salt("edit-rate").selected_text(format!("{} Hz", p.rate_hz)).show_ui(ui, |ui| {
                        for rate in proto::RATES { ui.selectable_value(&mut p.rate_hz,rate,format!("{rate} Hz")); }
                    }); ui.end_row();
                    ui.label(lang.text("Grip angle", "握持角度"));
                    let mut use_angle=p.rotation.is_some();
                    ui.horizontal(|ui| {ui.checkbox(&mut use_angle,lang.text("Set angle", "指定角度")); let mut angle=p.rotation.unwrap_or(0); if use_angle {ui.add(egui::DragValue::new(&mut angle).range(-30..=30).suffix("°"));} p.rotation=use_angle.then_some(angle);}); ui.end_row();
                    ui.label("LOD"); ui.horizontal(|ui| {ui.selectable_value(&mut p.lod,0,"1 mm");ui.selectable_value(&mut p.lod,1,"2 mm");});ui.end_row();
                    ui.label(lang.text("Debounce", "消抖时间"));ui.add(egui::DragValue::new(&mut p.debounce_ms).range(0..=30).suffix(" ms"));ui.end_row();
                    ui.label(lang.text("Sleep (0 = off)", "休眠（0 为关闭）"));ui.add(egui::DragValue::new(&mut p.sleep_min).range(0..=255).suffix(lang.text(" min", " 分钟")));ui.end_row();
                    ui.label(lang.text("Performance mode", "性能模式"));ui.add(egui::DragValue::new(&mut p.game_mode).range(1..=3));ui.end_row();
                });
                ui.separator();
                ui.checkbox(&mut p.motion_sync,lang.text("Motion Sync", "运动同步"));
                ui.checkbox(&mut p.ripple,lang.text("Ripple control", "波纹控制"));
                ui.checkbox(&mut p.angle_snap,lang.text("Angle snapping", "直线修正"));
                ui.separator();
                let mut include_system=p.system.is_some();
                ui.checkbox(&mut include_system,lang.text("Include system mouse settings", "包含系统鼠标设置"));
                let mut system=p.system.unwrap_or(mchose::system::Settings {speed:0.0,flat:true});
                if include_system {
                    ui.add(egui::Slider::new(&mut system.speed,-1.0..=1.0).text(lang.text("System speed", "系统速度")));
                    ui.checkbox(&mut system.flat,lang.text("Disable acceleration", "关闭鼠标加速度"));
                }
                p.system=include_system.then_some(system);
                ui.add_space(12.0);
                if let Some(error)=&self.error { ui.colored_label(BAD,error); }
                ui.horizontal(|ui| {
                    save=ui.add_enabled(!self.demo,egui::Button::new(lang.text("Save preset", "保存预设"))).clicked();
                    cancel=ui.button(lang.text("Cancel", "取消")).clicked();
                });
            });
        self.preset_editor = if open && !cancel {
            Some((name.clone(), p))
        } else {
            None
        };
        if save {
            match preset::save(&name, &p) {
                Ok(()) => {
                    self.presets = preset::all().into_iter().collect();
                    self.preset_editor = None;
                    self.error = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }
}

fn localized_event(event: &str, language: Language) -> String {
    match event {
        "Applied desktop preset" | "已切回 desktop 预设" => language
            .text("Applied desktop preset", "已切回 desktop 预设")
            .into(),
        "Stopped and applied desktop preset" | "已停止并切回 desktop 预设" => language
            .text(
                "Stopped and applied desktop preset",
                "已停止并切回 desktop 预设",
            )
            .into(),
        _ => {
            if let Some(details) = event
                .strip_prefix("Applied preset: ")
                .or_else(|| event.strip_prefix("已应用预设: "))
            {
                format!(
                    "{}: {details}",
                    language.text("Applied preset", "已应用预设")
                )
            } else {
                event.into()
            }
        }
    }
}
