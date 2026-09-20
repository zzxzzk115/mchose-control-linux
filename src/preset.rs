//! Named sets of settings, applied in as few round trips as the mouse allows.

use crate::hidraw::HidRaw;
use crate::proto::{self, Config, DPI_STAGES};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    /// Which of the six DPI stages this preset lives on, and what it holds.
    /// Presets ride the existing ladder rather than rewriting all six, so the
    /// DPI button on the mouse keeps working and nothing else is lost.
    pub stage: u8,
    pub dpi: u16,
    pub rate_hz: u32,
    /// 0 = 1 mm, 1 = 2 mm
    pub lod: u8,
    pub motion_sync: bool,
    pub ripple: bool,
    pub angle_snap: bool,
    pub debounce_ms: u8,
    pub sleep_min: u8,
    pub game_mode: u8,
    /// None preserves the user's angle (legacy and built-in presets).
    pub rotation: Option<i8>,
    /// Optional native KDE pointer settings; absent preserves system settings.
    pub system: Option<crate::system::Settings>,
}

/// What competitive Counter-Strike players run, on the mouse side.
///
/// 800 DPI and 1000 Hz is the settled pro standard. Everything that smooths,
/// predicts or delays is off: motion sync costs about a millisecond by pinning
/// sensor reads to the polling clock, ripple control is smoothing, and angle
/// snapping invents straight lines you did not draw. The lowest lift-off keeps
/// the crosshair still while you re-centre the mouse, and the mouse must never
/// sleep mid-round.
pub const CS: Preset = Preset {
    stage: 1,
    dpi: 800,
    rate_hz: 1000,
    lod: 0,
    motion_sync: false,
    ripple: false,
    angle_snap: false,
    debounce_ms: 3,
    sleep_min: 0,
    game_mode: 3,
    rotation: None,
    system: Some(crate::system::Settings {
        speed: 0.0,
        flat: true,
    }),
};

/// A day at the desk on a 4K panel.
///
/// The opposite trade: smoothing on, because pointing at a 3 px handle beats
/// three milliseconds of click latency; a slower poll and a real sleep timer,
/// because this preset runs all day on a battery; and a debounce high enough
/// that a tired switch never double-fires in a file manager.
pub const DESK: Preset = Preset {
    stage: 2,
    dpi: 1600,
    rate_hz: 500,
    lod: 1,
    motion_sync: true,
    ripple: true,
    angle_snap: false,
    debounce_ms: 10,
    sleep_min: 5,
    game_mode: 1,
    rotation: None,
    system: Some(crate::system::Settings {
        speed: 0.0,
        flat: false,
    }),
};

pub fn builtin() -> [(&'static str, Preset); 2] {
    [("cs2", CS), ("desktop", DESK)]
}

/// Built-ins plus anything saved, saved wins on a name clash.
pub fn all() -> BTreeMap<String, Preset> {
    merge_saved(load_saved())
}
fn merge_saved(saved: BTreeMap<String, Preset>) -> BTreeMap<String, Preset> {
    let mut out: BTreeMap<String, Preset> = builtin()
        .into_iter()
        .map(|(n, p)| (n.to_string(), p))
        .collect();
    for (name, value) in &saved {
        out.insert(canonical_name(name).to_owned(), *value);
    }
    // Explicit modern names win over legacy aliases.
    for (name, value) in saved {
        if canonical_name(&name) == name {
            out.insert(name, value);
        }
    }
    out
}

pub fn canonical_name(name: &str) -> &str {
    match name {
        "cs" => "cs2",
        "desk" => "desktop",
        _ => name,
    }
}

pub fn get(name: &str) -> Option<Preset> {
    let all = all();
    let name = name.to_lowercase();
    all.get(&name)
        .or_else(|| all.get(canonical_name(&name)))
        .copied()
}

/// Apply the whole preset. The block-borne settings go in one write; rate,
/// flags and game mode have their own commands and cannot be folded in.
pub fn apply(dev: &HidRaw, p: &Preset) -> io::Result<()> {
    validate(p)?;
    // Preflight the system backend before changing hardware.
    if p.system.is_some() {
        crate::system::current()?;
    }
    logln!("preset: applying {p:?}");
    let stage = (p.stage as usize).min(DPI_STAGES - 1);

    // Everything the config block carries goes in a single write, then the
    // three settings that have their own commands. One read, one block write,
    // one confirm: applying a preset used to take fifteen exchanges and choke
    // the mouse.
    let mut c = Config::read(dev)?;
    c.set_dpi(stage, p.dpi);
    c.set_dpi_stage(stage as u8);
    c.set_debounce_ms(p.debounce_ms);
    c.set_sleep_minutes(p.sleep_min);
    c.write(dev)?;
    let after = proto::confirm(dev, &c)?;

    if let Some(index) = proto::rate_index(p.rate_hz) {
        proto::set_report_rate(dev, index, index)?;
    }
    proto::set_flags_from(dev, &after, p.lod, p.ripple, p.angle_snap, p.motion_sync)?;
    proto::set_game_mode(dev, p.game_mode)?;
    if let Some(degrees) = p.rotation {
        proto::set_rotation(dev, degrees)?;
    }
    if let Some(settings) = p.system {
        crate::system::apply(settings)?;
    }
    let actual = current(dev)?;
    if !matches(p, &actual) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preset settings were not confirmed by the mouse",
        ));
    }
    Ok(())
}

/// How the mouse is set right now, so it can be saved or matched.
pub fn current(dev: &HidRaw) -> io::Result<Preset> {
    let c = Config::read(dev)?;
    let (ripple, angle_snap, motion_sync) = c.perf_flags();
    let link = proto::identity(dev)?.connect_mode;
    let stage = if link == 0 {
        c.wired_dpi_stage()
    } else {
        c.wireless_dpi_stage()
    }
    .min(DPI_STAGES as u8 - 1);
    Ok(Preset {
        stage,
        dpi: c.dpi(stage as usize),
        rate_hz: proto::rate_hz(if link == 0 {
            c.wired_rate_index()
        } else {
            c.wireless_rate_index()
        })
        .unwrap_or(0),
        lod: crate::proto::stored_lod(),
        motion_sync,
        ripple,
        angle_snap,
        debounce_ms: c.debounce_ms(),
        sleep_min: c.sleep_minutes(),
        game_mode: proto::basics(dev).map(|b| b.3).unwrap_or(0),
        rotation: Some(c.rotation()),
        system: crate::system::current().ok().map(|v| v.0),
    })
}

// ------------------------------------------------------------- saved presets

pub fn path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        });
    base.join("mchose/presets.conf")
}

fn load_saved() -> BTreeMap<String, Preset> {
    let Ok(text) = std::fs::read_to_string(path()) else {
        return BTreeMap::new();
    };
    parse_saved(&text)
}

fn parse_saved(text: &str) -> BTreeMap<String, Preset> {
    let mut out = BTreeMap::new();
    let mut name: Option<String> = None;
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut flush = |name: &mut Option<String>, fields: &mut BTreeMap<String, String>| {
        if let Some(n) = name.take() {
            let limits = [
                ("stage", 0, 5),
                ("dpi", 50, 26000),
                ("rate", 125, 8000),
                ("lod", 0, 1),
                ("motion_sync", 0, 1),
                ("ripple", 0, 1),
                ("angle_snap", 0, 1),
                ("debounce", 0, 30),
                ("sleep", 0, 255),
                ("game_mode", 1, 3),
                ("rotation", -30, 30),
            ];
            if validate_name(&n).is_err()
                || limits.iter().any(|(k, min, max)| {
                    fields
                        .get(*k)
                        .is_some_and(|v| v.parse::<i64>().map_or(true, |v| v < *min || v > *max))
                })
            {
                fields.clear();
                return;
            }
            let system =
                if fields.contains_key("system_speed") || fields.contains_key("system_flat") {
                    let value = fields.get("system_speed").and_then(|v| v.parse().ok()).zip(
                        fields.get("system_flat").and_then(|v| match v.as_str() {
                            "1" => Some(true),
                            "0" => Some(false),
                            _ => None,
                        }),
                    );
                    let Some((speed, flat)) = value else {
                        fields.clear();
                        return;
                    };
                    let setting = crate::system::Settings { speed, flat };
                    if setting.validate().is_err() {
                        fields.clear();
                        return;
                    }
                    Some(setting)
                } else {
                    None
                };
            let get = |k: &str, d: i64| fields.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
            out.insert(
                n,
                Preset {
                    stage: get("stage", 0) as u8,
                    dpi: get("dpi", 800) as u16,
                    rate_hz: get("rate", 1000) as u32,
                    lod: get("lod", 0) as u8,
                    motion_sync: get("motion_sync", 0) != 0,
                    ripple: get("ripple", 0) != 0,
                    angle_snap: get("angle_snap", 0) != 0,
                    debounce_ms: get("debounce", 8) as u8,
                    sleep_min: get("sleep", 3) as u8,
                    game_mode: get("game_mode", 1) as u8,
                    rotation: fields.get("rotation").and_then(|v| v.parse().ok()),
                    system,
                },
            );
        }
        fields.clear();
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            flush(&mut name, &mut fields);
            name = Some(section.trim().to_lowercase());
        } else if let Some((k, v)) = line.split_once('=') {
            fields.insert(k.trim().to_string(), v.trim().to_owned());
        }
    }
    flush(&mut name, &mut fields);
    out.retain(|_, p| validate(p).is_ok());
    out
}

pub fn save(name: &str, p: &Preset) -> io::Result<()> {
    validate_name(name)?;
    validate(p)?;
    let mut saved = load_saved();
    saved.insert(canonical_name(&name.trim().to_lowercase()).to_owned(), *p);
    write_saved(&saved)
}

pub fn remove(name: &str) -> io::Result<()> {
    let mut saved = load_saved();
    let name = name.to_lowercase();
    let canonical = canonical_name(&name);
    let mut removed = saved.remove(canonical).is_some();
    for legacy in ["cs", "desk"] {
        if canonical_name(legacy) == canonical {
            removed |= saved.remove(legacy).is_some();
        }
    }
    if !removed {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "saved preset not found (built-ins cannot be deleted)",
        ));
    }
    write_saved(&saved)
}

fn write_saved(saved: &BTreeMap<String, Preset>) -> io::Result<()> {
    let mut text = String::from("# mchose presets. Edit freely; a name here shadows a built-in.\n");
    for (n, p) in saved {
        text.push_str(&format!(
            "\n[{n}]\nstage={}\ndpi={}\nrate={}\nlod={}\nmotion_sync={}\nripple={}\nangle_snap={}\ndebounce={}\nsleep={}\ngame_mode={}\n",
            p.stage, p.dpi, p.rate_hz, p.lod, p.motion_sync as u8, p.ripple as u8,
            p.angle_snap as u8, p.debounce_ms, p.sleep_min, p.game_mode
        ));
        if let Some(settings) = p.system {
            text.push_str(&format!(
                "system_speed={}\nsystem_flat={}\n",
                settings.speed, settings.flat as u8
            ));
        }
        if let Some(angle) = p.rotation {
            text.push_str(&format!("rotation={angle}\n"));
        }
    }
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, text)?;
    std::fs::rename(temporary, path)
}

pub fn matches(expected: &Preset, actual: &Preset) -> bool {
    let mut normalized = *actual;
    if expected.rotation.is_none() {
        normalized.rotation = None;
    }
    if expected.system.is_none() {
        normalized.system = None;
    }
    *expected == normalized
}

pub fn validate_name(name: &str) -> io::Result<()> {
    if name.trim().is_empty()
        || name.len() > 80
        || name.chars().any(|c| c.is_control() || "[]=#".contains(c))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid preset name",
        ));
    }
    Ok(())
}

pub fn validate(p: &Preset) -> io::Result<()> {
    if let Some(s) = p.system {
        s.validate()?;
    }
    if p.stage >= 6
        || p.dpi < 50
        || p.dpi > 26000
        || p.dpi % 50 != 0
        || proto::rate_index(p.rate_hz).is_none()
        || p.lod > 1
        || p.debounce_ms > 30
        || !(1..=3).contains(&p.game_mode)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid preset settings",
        ));
    }
    if let Some(angle) = p.rotation {
        proto::validate_rotation(angle)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_names_migrate_without_overwriting_existing_cs2() {
        let old = Preset { dpi: 1200, ..CS };
        let custom = Preset {
            dpi: 2400,
            rotation: Some(-4),
            ..CS
        };
        let presets = merge_saved(BTreeMap::from([
            ("cs".into(), old),
            ("cs2".into(), custom),
            ("desk".into(), old),
        ]));
        assert_eq!(presets["cs2"], custom);
        assert_eq!(presets["desktop"], old);
        assert!(!presets.contains_key("cs"));
        assert!(!presets.contains_key("desk"));
        let migrated = merge_saved(BTreeMap::from([("cs".into(), old)]));
        assert_eq!(migrated["cs2"], old);
    }
    #[test]
    fn old_presets_preserve_rotation_and_new_presets_parse_signed_angles() {
        let old = parse_saved("[old]\ndpi=800\n");
        assert_eq!(old["old"].rotation, None);
        let new = parse_saved("[我的CS]\nrotation=-12\n");
        assert_eq!(new["我的cs"].rotation, Some(-12));
        let mut live = CS;
        live.rotation = Some(-8);
        assert!(matches(&CS, &live));
        assert!(!matches(
            &Preset {
                rotation: Some(2),
                ..CS
            },
            &live
        ));
    }
    #[test]
    fn invalid_numeric_values_cannot_wrap_or_silently_disable_rotation() {
        for input in [
            "[x]\nrotation=256",
            "[x]\ndpi=66336",
            "[x]\nrotation=31",
            "[x]\nrate=999",
        ] {
            assert!(parse_saved(input).is_empty());
        }
    }
    #[test]
    fn system_settings_are_optional_and_strict() {
        assert_eq!(parse_saved("[old]\ndpi=800")["old"].system, None);
        let p = parse_saved("[cs]\nsystem_speed=0.123456789\nsystem_flat=1");
        assert_eq!(
            p["cs"].system,
            Some(crate::system::Settings {
                speed: 0.123456789,
                flat: true
            })
        );
        for bad in [
            "system_speed=NaN\nsystem_flat=1",
            "system_speed=2\nsystem_flat=1",
            "system_speed=0",
            "system_flat=3\nsystem_speed=0",
            "dpi=abc",
        ] {
            assert!(parse_saved(&format!("[bad]\n{bad}")).is_empty());
        }
        assert!(!matches(
            &DESK,
            &Preset {
                system: CS.system,
                ..DESK
            }
        ));
        assert!(!matches(&CS, &Preset { system: None, ..CS }));
    }
    #[test]
    fn names_cannot_inject_sections() {
        assert!(validate_name("我的 CS").is_ok());
        assert!(validate_name("foo\n[bar]").is_err());
        assert!(validate_name(" ").is_err());
    }
}
