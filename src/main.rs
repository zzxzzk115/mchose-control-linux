//! mchose: configure MCHOSE mice on Linux.
//!
//! There is no vendor Linux app and libratbag does not know these mice. The
//! protocol was recovered from the M HUB web driver; PROTOCOL.md documents it.

use mchose::hidraw::{self, HidRaw};
use mchose::i18n::{self, Language};
use mchose::preset::{self, Preset};
use mchose::proto::{self, Config, DPI_STAGES, RATES};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
macro_rules! fmt_tr {
    ($en:literal, $zh:literal $(, $arg:expr)* $(,)?) => {
        if i18n::text("en", "zh") == "zh" { format!($zh $(, $arg)*) }
        else { format!($en $(, $arg)*) }
    };
}
macro_rules! out {
    ($en:literal, $zh:literal $(, $arg:expr)* $(,)?) => {
        if i18n::text("en", "zh") == "zh" { println!($zh $(, $arg)*); }
        else { println!($en $(, $arg)*); }
    };
}

/// Vendors the M HUB driver treats with this protocol.
const VENDORS: [u16; 2] = [0x5253, 0x3837];

const USAGE: &str = "\
mchose - configure MCHOSE mice on Linux

  mchose info                    device, firmware, battery, link
  mchose show [--raw]            DPI stages, report rate, debounce, sleep
  mchose devices                 hidraw nodes that speak this protocol

  mchose dpi <value>             set the active DPI stage
  mchose dpi <value> --stage N   set stage N (0-5)
  mchose dpi --list a,b,c,d,e,f  set all six stages at once
  mchose stage <0-5>             switch the active stage
  mchose rate <hz>               125 500 1000 2000 4000 8000
  mchose debounce <ms>
  mchose sleep <minutes>         0 disables
  mchose profile <0-2>

  mchose lod <mm>                lift-off distance, 1 or 2
  mchose motion-sync <on|off>
  mchose ripple <on|off>
  mchose angle-snap <on|off>
  mchose game-mode <1|2|3>
  mchose rotation [degrees]      read / set sensor rotation (-30..30)

  mchose backup [file]           save the config block
  mchose restore <file>          write a saved config block back
  mchose preset                  list the presets, mark the one in effect
  mchose preset cs2               competitive Counter-Strike
  mchose preset desktop             a day at the desk
  mchose preset save <name>      save settings including sensor rotation
  mchose preset delete <name>    remove a saved preset

  mchose log [lines]             the last frames sent and received
  mchose raw <11|12> <hex...>    send one frame, print the reply

  mchose auto                   foreground preset rules and monitoring

  mchose desktop status | autostart on|off | close-to-tray on|off
  mchose system show            system pointer settings (KDE)
  mchose system speed <-100..100>
  mchose system acceleration <on|off>
  mchose system cs              flat acceleration + neutral speed

Options: --device /dev/hidrawN   skip autodetection
         --lang auto|en|zh      language override
         --json                 machine-readable info / show
         --version              version information

Lift-off is the one setting the mouse never reports back, so the last value
written is kept in ~/.local/state/mchose/. Everything else is read from the
mouse itself.
";

const USAGE_ZH: &str = "迈从鼠标 · Linux 配置工具
用法：mchose [--lang auto|en|zh] [--device /dev/hidrawN] 命令

  info / show [--json]           设备信息 / 全部设置（可输出 JSON）
  devices                       列出可配置的鼠标
  rotation [角度]               读取或设置传感器旋转，范围 -30°～30°
  dpi <数值> [--stage 0-5]       设置当前或指定 DPI 档位
  dpi --list 400,800,1600,3200,6400,26000
  stage <0-5>                   切换 DPI 档位（命令行从 0 开始）
  rate <Hz>                     回报率：125 / 500 / 1000 / 2000 / 4000 / 8000
  lod <1|2>                     抬升高度（毫米，当前协议映射）
  debounce <0-30>               按键消抖（毫秒）
  sleep <分钟>                  休眠时间，0 为禁用
  motion-sync <on|off>           运动同步
  ripple <on|off>                波纹控制
  angle-snap <on|off>            直线修正（与握持角度不同）
  game-mode <1|2|3>             性能模式
  profile <0-2>                 鼠标板载配置

  preset                        查看预设
  preset <名称>                 应用预设
  preset save <名称>            保存当前设置，包含旋转角度
  preset delete <名称>          删除自定义预设
  backup [文件] / restore <文件> 备份 / 恢复配置
  log [行数]                    通信日志
  raw <11|12> <十六进制字节…>    调试协议

  auto                          前台应用规则与自动切换帮助

  desktop status                查看桌面驻留设置
  desktop autostart <on|off>     界面登录自启
  desktop close-to-tray <on|off>  关闭驻留托盘
  system show                   读取系统鼠标设置（KDE）
  system speed <-100..100>       系统指针速度，0 为默认
  system acceleration <on|off>   开启 / 关闭系统鼠标加速
  system cs                     关闭加速并设为默认速度

  --help                        帮助
  --version                     版本

示例：mchose --lang zh rotation -8
      mchose preset save 我的CS
内置 cs2 预设保留你的角度；自定义预设保存并恢复角度。
LOD 显示的是工具上次写入值，不能从设备读取。
";

fn main() -> ExitCode {
    // Behave like other CLI tools when a downstream pipe closes early.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mchose: {e}");
            ExitCode::FAILURE
        }
    }
}

type R = Result<(), String>;

fn run(args: &[String]) -> R {
    let mut args: Vec<String> = args.to_vec();
    if args.last().is_some_and(|v| v == "--lang") {
        return Err("--lang auto|en|zh（语言参数不可为空）".into());
    }
    let language = take_option(&mut args, "--lang");
    let language = match language {
        Some(v) => Language::parse(&v).ok_or(i18n::text(
            "--lang: auto / en / zh",
            "语言参数：--lang auto / en / zh",
        ))?,
        None => i18n::preference(),
    };
    i18n::init(language);
    let json = take_flag(&mut args, "--json");
    let explicit = take_option(&mut args, "--device");
    let raw_flag = take_flag(&mut args, "--raw");
    let stage_opt = take_option(&mut args, "--stage");
    let list_opt = take_option(&mut args, "--list");

    let command = args.first().map(String::as_str).unwrap_or("info");
    if matches!(command, "-h" | "--help" | "help") {
        print!("{}", i18n::text(USAGE, USAGE_ZH));
        return Ok(());
    }
    if command == "--version" {
        println!("mchose {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if json && !matches!(command, "info" | "show") {
        return Err(
            i18n::text("--json requires info or show", "--json 仅支持 info 或 show").into(),
        );
    }
    if command == "desktop" {
        match args.get(1).map(String::as_str).unwrap_or("status") {
            "status" => {}
            action @ ("autostart" | "close-to-tray") => {
                let value = match args.get(2).map(String::as_str) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("desktop autostart|close-to-tray on|off".into()),
                };
                if action == "autostart" {
                    mchose::desktop::set_autostart(value)
                } else {
                    mchose::desktop::set_close_to_tray(value)
                }
                .map_err(io)?;
            }
            _ => return Err("desktop status | autostart on|off | close-to-tray on|off".into()),
        }
        println!(
            "{}: {}\n{}: {}",
            i18n::text("GUI login startup", "界面登录自启"),
            mchose::desktop::autostart_path().exists(),
            i18n::text("Close to tray", "关闭驻留托盘"),
            mchose::desktop::close_to_tray()
        );
        return Ok(());
    }
    if command == "system" {
        let (mut settings, name) = mchose::system::current().map_err(io)?;
        match args.get(1).map(String::as_str).unwrap_or("show") {
            "show" => {}
            "speed" => {
                settings.speed = args
                    .get(2)
                    .ok_or("system speed <-100..100>")?
                    .parse::<f64>()
                    .map_err(|_| "Invalid speed")?
                    / 100.0;
                mchose::system::apply(settings).map_err(io)?;
            }
            "acceleration" => {
                settings.flat = match args.get(2).map(String::as_str) {
                    Some("off") => true,
                    Some("on") => false,
                    _ => return Err("system acceleration on|off".into()),
                };
                mchose::system::apply(settings).map_err(io)?;
            }
            "cs" => {
                settings = mchose::system::Settings {
                    speed: 0.0,
                    flat: true,
                };
                mchose::system::apply(settings).map_err(io)?;
            }
            _ => return Err("system show | speed <-100..100> | acceleration on|off | cs".into()),
        }
        println!(
            "{}: {}\n{}: {}\n{}: {}",
            i18n::text("System mouse", "系统鼠标"),
            name,
            i18n::text("Speed (-100..100)", "速度（-100～100）"),
            settings.speed * 100.0,
            i18n::text("Acceleration", "鼠标加速"),
            if settings.flat {
                i18n::text("Off (flat)", "关闭（恒定）")
            } else {
                i18n::text("On (adaptive)", "开启（自适应）")
            }
        );
        return Ok(());
    }
    if command == "preset" && args.get(1).map(String::as_str) == Some("delete") {
        let name = args.get(2).ok_or(i18n::text(
            "preset delete <name>",
            "用法：preset delete <名称>",
        ))?;
        preset::remove(name).map_err(io)?;
        out!("deleted preset {name}", "已删除预设 {name}");
        return Ok(());
    }
    if command == "rotation" {
        if let Some(v) = args.get(1) {
            let degrees: i8 = v.parse().map_err(|_| {
                i18n::text("rotation must be -30..30", "旋转角度必须为 -30～30 的整数")
            })?;
            proto::validate_rotation(degrees).map_err(io)?;
        }
    }
    if !matches!(
        command,
        "auto"
            | "info"
            | "show"
            | "devices"
            | "log"
            | "rotation"
            | "dpi"
            | "stage"
            | "rate"
            | "debounce"
            | "sleep"
            | "profile"
            | "lod"
            | "motion-sync"
            | "ripple"
            | "angle-snap"
            | "game-mode"
            | "backup"
            | "restore"
            | "preset"
            | "raw"
    ) {
        return Err(i18n::text("unknown command; see --help", "未知命令，请查看 --help").into());
    }
    if command == "auto" {
        return auto_cli(&args[1..]);
    }
    if command == "devices" {
        return devices();
    }
    if command == "log" {
        let text = fs::read_to_string(mchose::log::path()).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let n = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(60usize);
        for line in lines.iter().rev().take(n).rev() {
            println!("{line}");
        }
        return Ok(());
    }

    let dev = open_device(explicit.as_deref())?;

    if json {
        return json_state(&dev, command == "show");
    }
    match command {
        "rotation" => {
            if let Some(v) = args.get(1) {
                let degrees: i8 = v
                    .parse()
                    .map_err(|_| i18n::text("rotation -30..30", "角度范围为 -30～30"))?;
                backup_once(&dev)?;
                proto::set_rotation(&dev, degrees).map_err(io)?;
            }
            let degrees = Config::read(&dev).map_err(io)?.rotation();
            out!("sensor rotation {degrees:+}°", "传感器旋转 {degrees:+}°");
            Ok(())
        }
        "info" => info(&dev),
        "show" => show(&dev, raw_flag),
        "dpi" => {
            if let Some(list) = list_opt {
                dpi_list(&dev, &list)
            } else {
                let value: u16 = num(args.get(1), "dpi <value>")?;
                let stage =
                    match stage_opt {
                        Some(s) => Some(s.parse::<u8>().map_err(|_| {
                            i18n::text("--stage takes 0-5", "--stage 的范围为 0～5")
                        })?),
                        None => None,
                    };
                dpi(&dev, value, stage)
            }
        }
        "stage" => stage(&dev, num(args.get(1), "stage <0-5>")?),
        "rate" => rate(&dev, num(args.get(1), "rate <hz>")?),
        "debounce" => debounce(&dev, num(args.get(1), "debounce <ms>")?),
        "sleep" => sleep_minutes(&dev, num(args.get(1), "sleep <minutes>")?),
        "profile" => {
            let p: u8 = num(args.get(1), "profile <0-2>")?;
            if p > 2 {
                return Err(i18n::text("profile must be 0..2", "板载配置范围为 0～2").into());
            }
            backup_once(&dev)?;
            proto::set_profile(&dev, p).map_err(io)?;
            out!("profile {p}", "已切换板载配置 {p}");
            Ok(())
        }
        "lod" => {
            let mm: u8 = num(args.get(1), "lod <mm>")?;
            let index = match mm {
                1 => 0,
                2 => 1,
                _ => {
                    return Err(
                        i18n::text("lod takes 1 or 2 (mm)", "LOD 请输入 1 或 2（毫米）").into(),
                    )
                }
            };
            flags(&dev, |lod, _, _, _| *lod = index)?;
            out!("lift-off distance {mm} mm", "抬升高度 {mm} 毫米");
            Ok(())
        }
        "motion-sync" => {
            let on = onoff(args.get(1))? == 1;
            flags(&dev, |_, _, _, sync| *sync = on)?;
            out!("motion sync {}", "运动同步    {}", onoff_str(on));
            Ok(())
        }
        "ripple" => {
            let on = onoff(args.get(1))? == 1;
            flags(&dev, |_, r, _, _| *r = on)?;
            out!("ripple control {}", "波纹控制 {}", onoff_str(on));
            Ok(())
        }
        "angle-snap" => {
            let on = onoff(args.get(1))? == 1;
            flags(&dev, |_, _, a, _| *a = on)?;
            out!("angle snapping {}", "直线修正 {}", onoff_str(on));
            Ok(())
        }
        "game-mode" => {
            let mode: u8 = num(args.get(1), "game-mode <1|2|3>")?;
            if !(1..=3).contains(&mode) {
                return Err(
                    i18n::text("game-mode takes 1, 2 or 3", "性能模式请输入 1、2 或 3").into(),
                );
            }
            proto::set_game_mode(&dev, mode).map_err(io)?;
            std::thread::sleep(std::time::Duration::from_millis(60));
            let (_, _, _, got) = proto::basics(&dev).map_err(io)?;
            if got != mode {
                return Err(format!(
                    "the mouse did not take game mode {mode} (it reports {got})"
                ));
            }
            out!("game mode {mode}", "性能模式 {mode}");
            Ok(())
        }
        "backup" => backup(&dev, args.get(1).map(PathBuf::from)),
        "restore" => restore(
            &dev,
            args.get(1).ok_or(i18n::text(
                "usage: mchose restore <file>",
                "用法：mchose restore <文件>",
            ))?,
        ),
        "preset" => match args.get(1).map(String::as_str) {
            None => list_presets(&dev),
            Some("save") => {
                let name = args.get(2).ok_or(i18n::text(
                    "usage: mchose preset save <name>",
                    "用法：mchose preset save <名称>",
                ))?;
                let now = preset::current(&dev).map_err(io)?;
                preset::save(name, &now).map_err(io)?;
                out!(
                    "saved preset {name:?} to {}",
                    "已保存预设 {name:?}：{}",
                    preset::path().display()
                );
                Ok(())
            }
            Some(name) => {
                let p = preset::get(name).ok_or_else(|| {
                    fmt_tr!(
                        "no preset {name:?}. Try: mchose preset",
                        "没有预设 {name:?}。用 mchose preset 查看列表。"
                    )
                })?;
                backup_once(&dev)?;
                preset::apply(&dev, &p).map_err(io)?;
                println!("{name}");
                describe(&p, "  ");
                Ok(())
            }
        },
        "raw" => raw(&dev, &args[1..]),
        other => Err(fmt_tr!(
            "unknown command {other:?}, try --help",
            "未知命令 {other:?}，请查看 --help"
        )),
    }
}

// ------------------------------------------------------------------ device

fn candidates() -> Result<Vec<hidraw::Node>, String> {
    let nodes = hidraw::nodes().map_err(io)?;
    Ok(nodes
        .into_iter()
        .filter(|n| VENDORS.contains(&n.vid) && hidraw::has_config_collection(&n.descriptor))
        .collect())
}

fn devices() -> R {
    let found = candidates()?;
    if found.is_empty() {
        out!(
            "no MCHOSE configuration interface found",
            "未找到迈从鼠标配置接口"
        );
        return Ok(());
    }
    for n in found {
        println!(
            "{}  {:04x}:{:04x}  {}",
            n.dev.display(),
            n.vid,
            n.pid,
            n.name
        );
    }
    Ok(())
}

fn open_device(explicit: Option<&str>) -> Result<HidRaw, String> {
    if let Some(path) = explicit {
        return HidRaw::open(std::path::Path::new(path)).map_err(|e| open_hint(path, e));
    }
    let found = candidates()?;
    let node = found.first().ok_or(i18n::text(
        "no MCHOSE configuration interface found. Is the mouse or its dongle plugged in?",
        "未找到迈从鼠标配置接口，请连接鼠标或接收器。",
    ))?;
    HidRaw::open(&node.dev).map_err(|e| open_hint(&node.dev.to_string_lossy(), e))
}

fn open_hint(path: &str, e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        format!(
            "cannot open {path}: permission denied.\n\
             Install the udev rule once, then replug:\n  \
             sudo cp 70-mchose.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger"
        )
    } else {
        fmt_tr!("cannot open {path}: {e}", "无法打开 {path}：{e}")
    }
}

// ----------------------------------------------------------------- reading

fn info(dev: &HidRaw) -> R {
    let id = proto::identity(dev).map_err(io)?;
    let version = proto::version_string(dev).unwrap_or_default();
    out!(
        "device      {:04x}:{:04x}",
        "设备        {:04x}:{:04x}",
        id.vid,
        id.pid
    );
    if version.is_empty() {
        out!("firmware    {:#010x}", "固件        {:#010x}", id.firmware);
    } else {
        out!("firmware    {version}", "固件        {version}");
    }
    out!(
        "link        {} ({})",
        "连接        {}（{}）",
        link_name(id.connect_mode),
        if id.connected {
            i18n::text("online", "在线")
        } else {
            i18n::text("offline", "离线")
        }
    );
    out!(
        "battery     {} %{}",
        "电量        {} %{}",
        id.battery,
        if id.charging != 0 {
            i18n::text(", charging", "，充电中")
        } else {
            ""
        }
    );
    Ok(())
}

fn link_name(mode: u8) -> &'static str {
    match mode {
        0 => i18n::text("wired", "有线"),
        1 => "2.4 GHz",
        2 => i18n::text("bluetooth", "蓝牙"),
        _ => i18n::text("unknown", "未知"),
    }
}

fn show(dev: &HidRaw, raw_flag: bool) -> R {
    let c = Config::read(dev).map_err(io)?;
    let link = proto::identity(dev).map_err(io)?.connect_mode;
    let active = if link == 0 {
        c.wired_dpi_stage()
    } else {
        c.wireless_dpi_stage()
    } as usize;
    out!("profile     {}", "板载配置    {}", c.profile());
    out!(
        "stages      {} enabled",
        "DPI 档位    已启用 {} 档",
        c.enabled_stages()
    );
    for s in 0..DPI_STAGES {
        let mark = if s == active { "*" } else { " " };
        let y = c.dpi_y(s);
        let axis = if y != 0 && y != c.dpi(s) {
            format!("  (Y {y})")
        } else {
            String::new()
        };
        println!("  {mark} {s}       {} dpi{axis}", c.dpi(s));
    }
    out!(
        "rate        {} (wired), {} (wireless)",
        "回报率      {}（有线），{}（无线）",
        rate_name(c.wired_rate_index()),
        rate_name(c.wireless_rate_index())
    );
    out!("debounce    {} ms", "按键消抖    {} 毫秒", c.debounce_ms());
    out!(
        "sleep       {}",
        "休眠        {}",
        if c.sleep_minutes() == 0 {
            i18n::text("off", "关闭").to_string()
        } else {
            fmt_tr!("{} min", "{} 分钟", c.sleep_minutes())
        }
    );
    let (ripple, angle_snap, motion_sync) = c.perf_flags();
    out!("ripple      {}", "波纹控制    {}", onoff_str(ripple));
    out!("angle snap  {}", "直线修正    {}", onoff_str(angle_snap));
    out!("motion sync {}", "运动同步    {}", onoff_str(motion_sync));
    out!("sensor      {:#04x}", "传感器标志  {:#04x}", c.sensor());
    if let Ok((_, _, _, game)) = proto::basics(dev) {
        out!("game mode   {game}", "性能模式    {game}");
    }
    out!("rotation    {:+}°", "旋转角度    {:+}°", c.rotation());
    out!(
        "lod         {} mm (write-only, from this tool's state)",
        "抬升高度    {} 毫米（工具上次写入值，非设备读回）",
        if proto::stored_lod() == 0 { 1 } else { 2 }
    );
    if raw_flag {
        out!("\nraw config  {}", "\n配置原始数据 {}", proto::hex(&c.body));
    }
    Ok(())
}

fn rate_name(index: u8) -> String {
    RATES
        .get(index as usize)
        .map(|hz| format!("{hz} Hz"))
        .unwrap_or_else(|| fmt_tr!("index {index}", "索引 {index}"))
}

// ----------------------------------------------------------------- writing

/// Read, change, write back, then read again and confirm it landed.
fn edit<F: Fn(&mut Config)>(dev: &HidRaw, change: F) -> Result<Config, String> {
    backup_once(dev)?;
    let mut c = Config::read(dev).map_err(io)?;
    change(&mut c);
    c.write(dev).map_err(io)?;
    proto::confirm(dev, &c).map_err(io)
}

fn dpi(dev: &HidRaw, value: u16, stage: Option<u8>) -> R {
    check_dpi(value)?;
    let current = Config::read(dev).map_err(io)?;
    let link = proto::identity(dev).map_err(io)?.connect_mode;
    let active = if link == 0 {
        current.wired_dpi_stage()
    } else {
        current.wireless_dpi_stage()
    };
    let target = stage.unwrap_or(active) as usize;
    if target >= DPI_STAGES {
        return Err(fmt_tr!(
            "stage must be 0-{}",
            "DPI 档位必须在 0～{} 之间",
            DPI_STAGES - 1
        ));
    }
    edit(dev, |c| c.set_dpi(target, value))?;
    out!(
        "stage {target} set to {value} dpi",
        "DPI 档位 {target} 已设为 {value}"
    );
    Ok(())
}

fn dpi_list(dev: &HidRaw, list: &str) -> R {
    let values: Vec<u16> = list
        .split(',')
        .map(|v| {
            v.trim()
                .parse::<u16>()
                .map_err(|_| fmt_tr!("{v:?} is not a DPI value", "{v:?} 不是有效的 DPI 数值"))
        })
        .collect::<Result<_, _>>()?;
    if values.len() != DPI_STAGES {
        return Err(fmt_tr!(
            "--list takes exactly {DPI_STAGES} comma-separated values",
            "--list 需要 {DPI_STAGES} 个以逗号分隔的数值"
        ));
    }
    for v in &values {
        check_dpi(*v)?;
    }
    edit(dev, |c| {
        for (s, v) in values.iter().enumerate() {
            c.set_dpi(s, *v);
        }
        c.set_enabled_stages(DPI_STAGES as u8);
    })?;
    out!(
        "stages set to {}",
        "DPI 档位已设为 {}",
        values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

fn check_dpi(value: u16) -> R {
    if value < 50 || value > 26000 {
        return Err(fmt_tr!(
            "{value} dpi is outside the sensor range (50 to 26000)",
            "{value} DPI 超出当前支持范围（50～26000）"
        ));
    }
    if value % 50 != 0 {
        return Err(fmt_tr!(
            "{value} dpi is not a multiple of 50",
            "{value} DPI 不是 50 的倍数"
        ));
    }
    Ok(())
}

fn stage(dev: &HidRaw, n: u8) -> R {
    if n as usize >= DPI_STAGES {
        return Err(fmt_tr!(
            "stage must be 0-{}",
            "DPI 档位必须在 0～{} 之间",
            DPI_STAGES - 1
        ));
    }
    let c = edit(dev, |c| c.set_dpi_stage(n))?;
    out!(
        "active stage {n} ({} dpi)",
        "当前档位 {n}（{} DPI）",
        c.dpi(n as usize)
    );
    Ok(())
}

fn rate(dev: &HidRaw, hz: u32) -> R {
    let index = RATES.iter().position(|r| *r == hz).ok_or_else(|| {
        fmt_tr!(
            "rate must be one of {}",
            "回报率必须为以下数值之一：{}",
            RATES.map(|r| r.to_string()).join(", ")
        )
    })? as u8;
    backup_once(dev)?;
    // `0x11 0x41` owns the rate; the config block only mirrors it, so read it
    // back rather than writing both and letting them fight.
    proto::set_report_rate(dev, index, index).map_err(io)?;
    std::thread::sleep(std::time::Duration::from_millis(60));
    let c = Config::read(dev).map_err(io)?;
    if c.wired_rate_index() != index && c.wireless_rate_index() != index {
        return Err(format!(
            "the mouse did not take {hz} Hz (it still reports {} wired, {} wireless)",
            rate_name(c.wired_rate_index()),
            rate_name(c.wireless_rate_index())
        ));
    }
    out!("report rate {hz} Hz", "回报率 {hz} Hz");
    Ok(())
}

fn debounce(dev: &HidRaw, ms: u8) -> R {
    if ms > 30 {
        return Err(i18n::text("debounce is 0 to 30 ms", "按键消抖的范围为 0～30 毫秒").into());
    }
    edit(dev, |c| c.set_debounce_ms(ms))?;
    out!("debounce {ms} ms", "按键消抖 {ms} 毫秒");
    Ok(())
}

fn sleep_minutes(dev: &HidRaw, minutes: u8) -> R {
    edit(dev, |c| c.set_sleep_minutes(minutes))?;
    if minutes == 0 {
        out!("sleep disabled", "已禁用休眠");
    } else {
        out!("sleep after {minutes} min", "{minutes} 分钟后休眠");
    }
    Ok(())
}

// ------------------------------------------------------------------ backup

fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state")
        });
    base.join("mchose")
}

fn backup(dev: &HidRaw, to: Option<PathBuf>) -> R {
    let c = Config::read(dev).map_err(io)?;
    let path = to.unwrap_or_else(|| state_dir().join("config.bin"));
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io)?;
    }
    fs::write(&path, c.body).map_err(io)?;
    out!("saved {}", "已保存 {}", path.display());
    Ok(())
}

/// Keep one copy of the config as it was before this tool ever wrote to it.
fn backup_once(dev: &HidRaw) -> R {
    let path = state_dir().join("config.original.bin");
    if path.exists() {
        return Ok(());
    }
    let c = Config::read(dev).map_err(io)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io)?;
    }
    fs::write(&path, c.body).map_err(io)?;
    eprintln!(
        "{}",
        fmt_tr!(
            "Original configuration saved to {}",
            "原始配置已保存至 {}",
            path.display()
        )
    );
    Ok(())
}

fn restore(dev: &HidRaw, from: &str) -> R {
    let body = fs::read(from).map_err(io)?;
    if body.len() != 63 {
        return Err(fmt_tr!(
            "{from} is {} bytes, expected 63",
            "{from} 为 {} 字节，应为 63 字节",
            body.len()
        ));
    }
    let mut c = Config::read(dev).map_err(io)?;
    c.body.copy_from_slice(&body);
    c.write(dev).map_err(io)?;
    proto::confirm(dev, &c).map_err(io)?;
    out!("restored from {from}", "已从 {from} 恢复");
    Ok(())
}

// ------------------------------------------------------------------- flags

/// The four sensor flags travel together in one command, so read the live set,
/// change the one asked for, and send them all.
fn flags<F: Fn(&mut u8, &mut bool, &mut bool, &mut bool)>(dev: &HidRaw, change: F) -> R {
    let c = Config::read(dev).map_err(io)?;
    let (mut ripple, mut angle_snap, mut motion_sync) = c.perf_flags();
    let mut lod = proto::stored_lod();
    change(&mut lod, &mut ripple, &mut angle_snap, &mut motion_sync);
    proto::set_flags(dev, lod, ripple, angle_snap, motion_sync).map_err(io)
}

// ----------------------------------------------------------------- presets

fn list_presets(dev: &HidRaw) -> R {
    let now = preset::current(dev).ok();
    for (name, p) in preset::all() {
        let live = now
            .as_ref()
            .is_some_and(|actual| preset::matches(&p, actual));
        println!("{} {name}", if live { "*" } else { " " });
        describe(&p, "    ");
        println!();
    }
    if now.is_none() {
        return Ok(());
    }
    if !preset::all().values().any(|p| {
        now.as_ref()
            .is_some_and(|actual| preset::matches(p, actual))
    }) {
        out!(
            "  the mouse is on none of these. `mchose preset save <name>` keeps it.",
            "  当前为自定义设置，可用 mchose preset save <名称> 保存。"
        );
    }
    Ok(())
}

fn describe(p: &Preset, pad: &str) {
    let angle = p
        .rotation
        .map(|v| format!("{v:+}°"))
        .unwrap_or_else(|| i18n::text("preserve", "保留当前角度").into());
    out!("{pad}rotation {angle}", "{pad}旋转角度 {angle}");
    out!(
        "{pad}{} dpi on stage {}, {} Hz",
        "{pad}{} DPI，档位 {}，{} Hz",
        p.dpi,
        p.stage,
        p.rate_hz
    );
    out!(
        "{pad}lift-off {} mm, motion sync {}, ripple {}, angle snap {}",
        "{pad}抬升 {} 毫米，运动同步 {}，波纹控制 {}，直线修正 {}",
        if p.lod == 0 { 1 } else { 2 },
        onoff_str(p.motion_sync),
        onoff_str(p.ripple),
        onoff_str(p.angle_snap)
    );
    out!(
        "{pad}debounce {} ms, sleep {}, game mode {}",
        "{pad}消抖 {} 毫秒，休眠 {}，性能模式 {}",
        p.debounce_ms,
        if p.sleep_min == 0 {
            i18n::text("off", "关闭").into()
        } else {
            fmt_tr!("{} min", "{} 分钟", p.sleep_min)
        },
        p.game_mode
    );
}

// --------------------------------------------------------------------- raw

fn raw(dev: &HidRaw, args: &[String]) -> R {
    let report = args
        .first()
        .and_then(|v| u8::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .ok_or(i18n::text(
            "raw <11|12> <hex bytes...>",
            "用法：raw <11|12> <十六进制字节…>",
        ))?;
    let body: Vec<u8> = args[1..]
        .iter()
        .map(|v| {
            u8::from_str_radix(v.trim_start_matches("0x"), 16)
                .map_err(|_| fmt_tr!("{v:?} is not a hex byte", "{v:?} 不是十六进制字节"))
        })
        .collect::<Result<_, _>>()?;
    if body.len() > proto::payload_len(report) || !matches!(report, 0x11 | 0x12) {
        return Err(i18n::text(
            "invalid raw report or payload length",
            "调试报告编号或数据长度无效",
        )
        .into());
    }
    if body.is_empty() {
        return Err(i18n::text("give at least the command byte", "请至少提供命令字节").into());
    }
    match proto::request(dev, report, &body) {
        Ok(reply) => println!("{}", proto::hex(&reply)),
        Err(e) => {
            proto::send(dev, report, &body).map_err(io)?;
            out!(
                "sent, no matching reply ({e})",
                "已发送，但未收到匹配回复（{e}）"
            );
        }
    }
    Ok(())
}

// ------------------------------------------------------------------- utils

fn onoff_str(v: bool) -> &'static str {
    if v {
        i18n::text("on", "开启")
    } else {
        i18n::text("off", "关闭")
    }
}

fn io(e: std::io::Error) -> String {
    format!(
        "{}: {}",
        i18n::text("Device / file operation failed", "设备或文件操作失败"),
        i18n::error_text(&e.to_string(), i18n::text("en", "zh") == "zh")
    )
}

fn num<T: std::str::FromStr>(arg: Option<&String>, what: &str) -> Result<T, String> {
    arg.ok_or_else(|| fmt_tr!("usage: mchose {what}", "用法：mchose {what}"))?
        .parse()
        .map_err(|_| fmt_tr!("usage: mchose {what}", "用法：mchose {what}"))
}

fn onoff(arg: Option<&String>) -> Result<u8, String> {
    match arg.map(String::as_str) {
        Some("on" | "1" | "true") => Ok(1),
        Some("off" | "0" | "false") => Ok(0),
        _ => Err(i18n::text("takes on or off", "请输入 on 或 off").into()),
    }
}

fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    if let Some(i) = args.iter().position(|a| a == name) {
        args.remove(i);
        true
    } else {
        false
    }
}

fn take_option(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args
        .iter()
        .position(|a| a == name || a.starts_with(&format!("{name}=")))?;
    let arg = args.remove(i);
    if let Some(v) = arg.strip_prefix(&format!("{name}=")) {
        return Some(v.to_string());
    }
    if i < args.len() {
        Some(args.remove(i))
    } else {
        None
    }
}

fn json_string(s: &str) -> String {
    let mut result = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            c if c.is_control() => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}
fn json_state(dev: &HidRaw, settings: bool) -> R {
    let id = proto::identity(dev).map_err(io)?;
    let version = proto::version_string(dev).map_err(io)?;
    let mut fields = vec![
        format!("\"vendor_id\":{}", id.vid),
        format!("\"product_id\":{}", id.pid),
        format!("\"firmware\":{}", json_string(&version)),
        format!("\"battery_percent\":{}", id.battery),
        format!("\"connected\":{}", id.connected),
        format!("\"connection_mode\":{}", id.connect_mode),
    ];
    if settings {
        let c = Config::read(dev).map_err(io)?;
        let (ripple, snap, sync) = c.perf_flags();
        fields.extend([
            format!(
                "\"dpi_stages\":[{}]",
                (0..6)
                    .map(|s| c.dpi(s).to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            format!(
                "\"active_stage\":{}",
                if id.connect_mode == 0 {
                    c.wired_dpi_stage()
                } else {
                    c.wireless_dpi_stage()
                }
            ),
            format!("\"rotation_degrees\":{}", c.rotation()),
            format!(
                "\"wired_rate_hz\":{}",
                proto::rate_hz(c.wired_rate_index())
                    .map(|v| v.to_string())
                    .unwrap_or("null".into())
            ),
            format!(
                "\"wireless_rate_hz\":{}",
                proto::rate_hz(c.wireless_rate_index())
                    .map(|v| v.to_string())
                    .unwrap_or("null".into())
            ),
            format!("\"debounce_ms\":{}", c.debounce_ms()),
            format!("\"sleep_minutes\":{}", c.sleep_minutes()),
            format!("\"ripple\":{ripple}"),
            format!("\"angle_snap\":{snap}"),
            format!("\"motion_sync\":{sync}"),
            format!("\"lod_cached_index\":{}", proto::stored_lod()),
        ]);
    }
    println!("{{{}}}", fields.join(","));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_strings_escape_controls_but_keep_unicode() {
        assert_eq!(json_string("中文\n\"\\"), "\"中文\\u000a\\\"\\\\\"");
    }
    #[test]
    fn missing_language_argument_is_an_error_without_device_io() {
        assert!(run(&["--lang".into()]).is_err());
    }
}

fn auto_cli(args: &[String]) -> R {
    use mchose::auto;
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "help" | "--help" => {
            out!("Automatic foreground presets (KDE Plasma 6)\n  auto apps [search] | bind-app <application-id> <preset>\n  auto bind <preset> '<filter>'\n  auto rules | remove <number> | enable <number> | disable <number>\n  auto start | stop | status | inspect | events | notify-test\n  auto test '<filter>'\n  auto autostart <on|off>\nFields: app_id, class, exe, path, title. Globs: * ?. Operators: && || ! ( ).\nExample: mchose auto bind cs2 'app_id=steam_app_730 || exe=cs2'\nFirst enabled match wins. Unmatched focus applies the desktop preset.",
        "前台应用自动预设（KDE Plasma 6）\n  auto apps [搜索词] | bind-app <应用ID> <预设>\n  auto bind <预设> '<过滤规则>'\n  auto rules | remove <序号> | enable <序号> | disable <序号>\n  auto start | stop | status | inspect | events | notify-test\n  auto test '<过滤规则>'\n  auto autostart <on|off>\n字段：app_id、class、exe、path、title。通配符：* ?。逻辑：&& || ! ( )。\n示例：mchose auto bind cs2 'app_id=steam_app_730 || exe=cs2'\n第一条已启用且匹配的规则优先；切出匹配应用后应用 desktop 默认预设。");
        }
        "apps" => {
            let query = args.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
            let lang = if i18n::text("en", "zh") == "zh" {
                Language::Chinese
            } else {
                Language::English
            };
            for app in mchose::applications::installed() {
                if format!("{} {} {}", app.name, app.chinese_name, app.id)
                    .to_lowercase()
                    .contains(&query)
                {
                    println!("{}\t{}", app.id, app.label(lang));
                }
            }
        }
        "bind-app" => {
            let id = args
                .get(1)
                .ok_or("auto bind-app <application-id> <preset>")?;
            let name = args
                .get(2)
                .ok_or("auto bind-app <application-id> <preset>")?;
            if preset::get(name).is_none() {
                return Err(i18n::text("Preset not found", "未找到预设").into());
            }
            let app = mchose::applications::installed()
                .into_iter()
                .find(|a| &a.id == id)
                .ok_or_else(|| {
                    i18n::text(
                        "Application not found; see auto apps",
                        "未找到应用，请使用 auto apps 查看应用列表",
                    )
                })?;
            let mut rules = auto::rules().map_err(io)?;
            rules.push(auto::Rule {
                enabled: true,
                preset: name.to_lowercase(),
                filter: app.filter,
            });
            auto::save_rules(&rules).map_err(io)?;
            out!("Application binding saved.", "应用绑定已保存。");
        }
        "bind" => {
            let name = args.get(1).ok_or("auto bind <preset> '<filter>'")?;
            let filter = args.get(2).ok_or("auto bind <preset> '<filter>'")?;
            if preset::get(name).is_none() {
                return Err(i18n::text("Preset not found", "未找到预设").into());
            }
            let mut rules = auto::rules().map_err(io)?;
            rules.push(auto::Rule {
                enabled: true,
                preset: name.to_lowercase(),
                filter: filter.clone(),
            });
            auto::save_rules(&rules).map_err(io)?;
            out!(
                "Rule saved. Run mchose auto start to enable monitoring.",
                "规则已保存，运行 mchose auto start 开始监听。"
            );
        }
        "rules" => {
            for (i, r) in auto::rules().map_err(io)?.iter().enumerate() {
                println!(
                    "{}  [{}] {} ← {}",
                    i + 1,
                    onoff_str(r.enabled),
                    r.preset,
                    r.filter
                );
            }
        }
        "remove" | "enable" | "disable" => {
            let n: usize = num(args.get(1), "auto remove|enable|disable <number>")?;
            let mut rules = auto::rules().map_err(io)?;
            if n == 0 || n > rules.len() {
                return Err(i18n::text("Rule number out of range", "规则序号超出范围").into());
            }
            if cmd == "remove" {
                rules.remove(n - 1);
            } else {
                rules[n - 1].enabled = cmd == "enable";
            }
            auto::save_rules(&rules).map_err(io)?;
        }
        "start" => {
            auto::start().map_err(io)?;
            out!(
                "Automatic switching requested. Check auto status.",
                "已请求启动自动切换，可用 auto status 查看状态。"
            );
        }
        "stop" => {
            auto::stop().map_err(io)?;
            out!(
                "Stopping; the desktop preset will be applied.",
                "正在停止，将应用 desktop 默认预设。"
            );
        }
        "run" => auto::run().map_err(io)?,
        "events" => print!("{}", mchose::notifications::history()),
        "notify-test" => mchose::notifications::send(
            "MCHOSE Control",
            i18n::text("Desktop notification test", "桌面通知测试"),
        )
        .map_err(io)?,
        "status" => {
            let s = auto::status();
            out!(
                "Running: {} · state: {} · preset: {}",
                "运行：{} · 状态：{} · 预设：{}",
                onoff_str(auto::running()),
                s.state,
                s.active
            );
            if !s.last_event.is_empty() {
                println!("{}", s.last_event);
            }
            if !s.message.is_empty() {
                println!("{}", s.message);
            }
            println!(
                "app_id={}  class={}  exe={}",
                s.window.app_id, s.window.class, s.window.exe
            );
        }
        "inspect" | "test" => {
            let w = if auto::running() {
                auto::status().window
            } else {
                auto::inspect().map_err(io)?
            };
            println!(
                "app_id={}\nclass={}\nexe={}\npath={}\ntitle={}",
                w.app_id, w.class, w.exe, w.path, w.title
            );
            if cmd == "test" {
                let filter = args.get(1).ok_or("auto test '<filter>'")?;
                out!(
                    "Matches: {}",
                    "是否匹配：{}",
                    onoff_str(auto::matches(filter, &w).map_err(io)?)
                );
            }
        }
        "autostart" => {
            auto::set_autostart(onoff(args.get(1))? == 1).map_err(io)?;
        }
        _ => {
            return Err(i18n::text(
                "Unknown auto command; use mchose auto",
                "未知自动切换命令，请运行 mchose auto 查看帮助",
            )
            .into())
        }
    }
    Ok(())
}
