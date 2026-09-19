//! KDE's native per-device libinput settings. No shell, XWayland or global fallback.
use std::{io, process::Command};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub speed: f64,
    pub flat: bool,
}
impl Settings {
    pub fn validate(self) -> io::Result<()> {
        if !self.speed.is_finite() || !(-1.0..=1.0).contains(&self.speed) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "System speed must be -1..1",
            ));
        }
        Ok(())
    }
}
fn run(settings: Option<Settings>) -> io::Result<(Settings, String)> {
    let mut cmd = Command::new("python3");
    cmd.arg("-c")
        .arg(include_str!("../integrations/system-mouse.py"));
    if let Some(s) = settings {
        s.validate()?;
        cmd.arg(s.speed.to_string()).arg(s.flat.to_string());
    }
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ));
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}
fn parse(text: &str) -> io::Result<(Settings, String)> {
    let mut fields = text.trim().splitn(3, '\t');
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid system mouse response");
    let speed = fields
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let flat = fields
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let name = fields.next().ok_or_else(invalid)?.to_owned();
    let s = Settings { speed, flat };
    s.validate()?;
    Ok((s, name))
}
pub fn current() -> io::Result<(Settings, String)> {
    run(None)
}
pub fn apply(s: Settings) -> io::Result<()> {
    run(Some(s)).map(|_| ())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_validation() {
        for v in [f64::NAN, f64::INFINITY, -1.1, 1.01] {
            assert!(Settings {
                speed: v,
                flat: true
            }
            .validate()
            .is_err());
        }
        assert!(parse("0.35\ttrue\tMouse").unwrap().0.flat);
        assert!(parse("0\tmaybe\tMouse").is_err());
    }
}
