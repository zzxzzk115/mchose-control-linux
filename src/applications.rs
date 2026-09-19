//! Installed application catalogue. Desktop launch commands are parsed, never executed.
use crate::{auto, i18n::Language};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub chinese_name: String,
    pub filter: String,
}
impl Application {
    pub fn label(&self, language: Language) -> &str {
        if language.resolve() == Language::Chinese && !self.chinese_name.is_empty() {
            &self.chinese_name
        } else {
            &self.name
        }
    }
}
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
fn term(field: &str, value: &str) -> String {
    format!("{field}={}", quote(value))
}
fn command_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    for c in s.chars() {
        if escape {
            word.push(c);
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c.is_whitespace() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}
fn game_filter(id: &str) -> String {
    let mut filter = format!("app_id=steam_app_{id} || class=steam_app_{id}");
    if id == "730" {
        filter.push_str(" || exe=cs2");
    }
    filter
}
fn steam_id(exec: &str) -> Option<String> {
    for prefix in ["steam://rungameid/", "steam://run/"] {
        if let Some((_, tail)) = exec.split_once(prefix) {
            let id: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}
fn parse_desktop(id: &str, text: &str, desktop: &str) -> Option<Application> {
    let mut fields = BTreeMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry && !line.starts_with('#') {
            if let Some((k, v)) = line.split_once('=') {
                fields.insert(k, v);
            }
        }
    }
    let get = |key| fields.get(key).copied().unwrap_or("");
    if get("Type") != "Application"
        || get("Hidden") == "true"
        || get("NoDisplay") == "true"
        || get("Name").is_empty()
    {
        return None;
    }
    let desktops: Vec<_> = desktop.split(':').collect();
    if !get("OnlyShowIn").is_empty() && !get("OnlyShowIn").split(';').any(|d| desktops.contains(&d))
    {
        return None;
    }
    if get("NotShowIn")
        .split(';')
        .filter(|d| !d.is_empty())
        .any(|d| desktops.contains(&d))
    {
        return None;
    }
    let exec = get("Exec");
    let app_id = id.trim_end_matches(".desktop");
    let filter = if let Some(game) = steam_id(exec) {
        game_filter(&game)
    } else {
        let mut terms = vec![term("app_id", app_id)];
        if !get("StartupWMClass").is_empty() {
            terms.push(term("class", get("StartupWMClass")));
        }
        let words = command_words(exec);
        if let Some(command) = words.first() {
            let exe = Path::new(command)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            // Generic launchers and web-app hosts must not match unrelated applications.
            let generic = [
                "env", "sh", "bash", "zsh", "fish", "python", "python3", "node", "electron",
                "flatpak", "snap", "steam", "wine", "wine64", "java",
            ];
            if !exe.is_empty()
                && !generic.contains(&exe)
                && !words.iter().any(|w| w.starts_with("--app"))
            {
                terms.push(term("exe", exe));
            }
        }
        terms.join(" || ")
    };
    if auto::validate_filter(&filter).is_err() {
        return None;
    }
    Some(Application {
        id: app_id.into(),
        name: get("Name").replace("\\s", " "),
        chinese_name: if !get("Name[zh_CN]").is_empty() {
            get("Name[zh_CN]")
        } else {
            get("Name[zh]")
        }
        .into(),
        filter,
    })
}
fn desktop_files(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>, depth: usize) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            desktop_files(base, &path, out, depth + 1);
        } else if path.extension().is_some_and(|s| s == "desktop") {
            if let Ok(relative) = path.strip_prefix(base) {
                out.push((relative.to_string_lossy().replace('/', "-"), path));
            }
        }
    }
}
fn vdf_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let words = command_words(line);
        if words.first().is_some_and(|w| w == key) {
            words.get(1).cloned()
        } else {
            None
        }
    })
}
pub fn installed() -> Vec<Application> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let mut bases = vec![data_home.join("applications")];
    bases.extend(
        std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
            .split(':')
            .filter(|p| !p.is_empty())
            .map(|p| PathBuf::from(p).join("applications")),
    );
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for base in bases {
        let mut files = Vec::new();
        desktop_files(&base, &base, &mut files, 0);
        files.sort();
        for (id, path) in files {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(path) {
                if let Some(app) = parse_desktop(&id, &text, &desktop) {
                    result.push(app);
                }
            }
        }
    }
    let mut libraries =
        BTreeSet::from([home.join(".local/share/Steam"), home.join(".steam/steam")]);
    for steam in libraries.clone() {
        if let Ok(text) = fs::read_to_string(steam.join("steamapps/libraryfolders.vdf")) {
            for line in text.lines() {
                if let Some(path) = vdf_value(line, "path") {
                    libraries.insert(PathBuf::from(path));
                }
            }
        }
    }
    for library in libraries {
        let Ok(entries) = fs::read_dir(library.join("steamapps")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with("appmanifest_")
                || path.extension().is_none_or(|s| s != "acf")
            {
                continue;
            }
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            let (Some(id), Some(name)) = (vdf_value(&text, "appid"), vdf_value(&text, "name"))
            else {
                continue;
            };
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let filter = game_filter(&id);
            if result.iter().any(|a| a.filter == filter) {
                continue;
            }
            result.push(Application {
                id: format!("steam:{id}"),
                name,
                chinese_name: String::new(),
                filter,
            });
        }
    }
    result.sort_by_key(|a| a.name.to_lowercase());
    result
}
/// A recently observed window can be chosen without typing identifiers.
pub fn observed(window: &auto::Window) -> Option<Application> {
    let mut terms = Vec::new();
    if !window.app_id.is_empty() {
        terms.push(term("app_id", &window.app_id));
    }
    if !window.class.is_empty() {
        terms.push(term("class", &window.class));
    }
    if terms.is_empty() && !window.exe.is_empty() {
        terms.push(term("exe", &window.exe));
    }
    if terms.is_empty() {
        return None;
    }
    let name = if window.title.is_empty() {
        window.exe.clone()
    } else {
        window.title.chars().take(80).collect()
    };
    Some(Application {
        id: "recent".into(),
        name,
        chinese_name: String::new(),
        filter: terms.join(" || "),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_desktop_selection_generates_rules() {
        let a=parse_desktop("org.kde.konsole.desktop","[Desktop Entry]\nType=Application\nName=Konsole\nName[zh_CN]=终端\nExec=konsole %U\nStartupWMClass=konsole\n","KDE").unwrap();
        assert_eq!(a.label(Language::Chinese), "终端");
        assert!(auto::matches(
            &a.filter,
            &auto::Window {
                app_id: "org.kde.konsole".into(),
                ..Default::default()
            }
        )
        .unwrap());
    }
    #[test]
    fn steam_game_does_not_match_the_entire_steam_client() {
        let a = parse_desktop(
            "CS2.desktop",
            "[Desktop Entry]\nType=Application\nName=CS2\nExec=steam steam://rungameid/730\n",
            "KDE",
        )
        .unwrap();
        assert!(!auto::matches(
            &a.filter,
            &auto::Window {
                exe: "steam".into(),
                ..Default::default()
            }
        )
        .unwrap());
        assert!(auto::matches(
            &a.filter,
            &auto::Window {
                exe: "cs2".into(),
                ..Default::default()
            }
        )
        .unwrap());
    }
    #[test]
    fn hidden_and_generic_launcher_entries_are_handled() {
        assert!(parse_desktop(
            "x.desktop",
            "[Desktop Entry]\nType=Application\nName=x\nHidden=true\n",
            "KDE"
        )
        .is_none());
        let a = parse_desktop(
            "org.app.desktop",
            "[Desktop Entry]\nType=Application\nName=App\nExec=flatpak run org.app\n",
            "KDE",
        )
        .unwrap();
        assert!(!auto::matches(
            &a.filter,
            &auto::Window {
                exe: "flatpak".into(),
                ..Default::default()
            }
        )
        .unwrap());
    }
}
