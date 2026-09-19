//! Shared locale selection. Command names and machine-readable output stay stable.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
static CHINESE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Auto,
    English,
    Chinese,
}
impl Language {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "en" | "en-us" | "english" => Some(Self::English),
            "zh" | "zh-cn" | "zh_cn" | "中文" => Some(Self::Chinese),
            _ => None,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::English => "en",
            Self::Chinese => "zh",
        }
    }
    pub fn resolve(self) -> Self {
        if self != Self::Auto {
            return self;
        }
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        if locale.to_lowercase().starts_with("zh") {
            Self::Chinese
        } else {
            Self::English
        }
    }
    pub fn text<'a>(self, en: &'a str, zh: &'a str) -> &'a str {
        if self.resolve() == Self::Chinese {
            zh
        } else {
            en
        }
    }
}
pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("mchose/language")
}
pub fn preference() -> Language {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| Language::parse(s.trim()))
        .unwrap_or(Language::Chinese)
}
pub fn save(language: Language) -> std::io::Result<()> {
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, language.code())
}
pub fn init(language: Language) {
    CHINESE.store(language.resolve() == Language::Chinese, Ordering::Relaxed);
}
pub fn text<'a>(en: &'a str, zh: &'a str) -> &'a str {
    if CHINESE.load(Ordering::Relaxed) {
        zh
    } else {
        en
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_languages_do_not_depend_on_locale() {
        assert_eq!(Language::parse("zh-CN"), Some(Language::Chinese));
        assert_eq!(Language::English.text("Angle", "角度"), "Angle");
        assert_eq!(Language::Chinese.text("Angle", "角度"), "角度");
        assert_eq!(Language::parse("xx"), None);
    }
}

/// Friendly translations for shared device errors; unknown OS diagnostics stay intact.
pub fn error_text(message: &str, chinese: bool) -> String {
    if !chinese {
        return message.to_owned();
    }
    let translated = match message {
        "preset settings were not confirmed by the mouse" => {
            "鼠标未确认预设的全部参数，请刷新查看实际状态"
        }
        "filter is too long" => "过滤规则过长（最多 1024 字节）",
        "use && or ||" => "请使用 && 或 || 连接条件",
        "incomplete escape" => "转义字符不完整",
        "unclosed quote" => "双引号未闭合",
        "missing filter term" => "缺少匹配条件",
        "missing )" => "缺少右括号 )",
        "use field=glob, e.g. app_id=steam_app_730" => {
            "请使用 字段=通配模式，例如 app_id=steam_app_730"
        }
        "fields: app_id, class, exe, path, title; pattern cannot be empty" => {
            "字段可用 app_id、class、exe、path、title，匹配值不可为空"
        }
        "unexpected filter operator" | "unexpected filter term" => {
            "过滤规则中的运算符或条件位置不正确"
        }
        "rule refers to a missing preset" => "规则引用的预设不存在，请修改规则",
        "KDE focus bridge stopped; requires KDE Plasma 6, Python 3 and PyGObject" => {
            "KDE 前台监听已停止，需要 KDE Plasma 6、Python 3 和 PyGObject"
        }
        "KWin did not report an active window" => "KWin 未返回前台窗口信息",
        "KDE focus bridge did not report a window" => "KDE 监听未返回窗口信息",
        "mouse changed; refusing to restore another device" => {
            "鼠标设备已更换，不能把旧配置恢复到另一只鼠标"
        }

        "mouse is busy in another MCHOSE process; retry after it finishes" => {
            "鼠标正由另一个 MCHOSE 进程操作，请稍后重试"
        }
        "rotation must be between -30 and 30 degrees" => "旋转角度必须在 -30° 到 +30° 之间",
        "rotation was not confirmed by the mouse" => "鼠标未确认旋转角度，请刷新后重试",
        "No MCHOSE mouse found. Connect the mouse or receiver." => {
            "未找到迈从鼠标，请连接鼠标或接收器"
        }
        "Polling rate was not confirmed by the mouse" => "鼠标未确认回报率，请刷新后重试",
        "Performance mode was not confirmed" => "鼠标未确认性能模式，请刷新后重试",
        "Preset not found" => "未找到预设",
        "invalid preset name" => "预设名称无效，请避免换行、方括号、等号和井号",
        "invalid preset settings" => "预设包含不支持的参数值",
        "saved preset not found (built-ins cannot be deleted)" => {
            "没有同名自定义预设，内置预设不能删除"
        }
        "Device worker stopped. Reopen the application." => "设备通信已停止，请重新打开应用",
        "the mouse would not take those flags" => "鼠标未接受这些传感器设置",
        _ => return message.to_owned(),
    };
    translated.to_owned()
}
