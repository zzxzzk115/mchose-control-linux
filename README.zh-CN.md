# 中文使用指南

[项目首页](README.md) · [English](README.en.md)

非官方开源工具。0.3 版提供中英文 GUI、命令行、传感器旋转、系统鼠标设置及托盘驻留。

## 安装与启动

需要 Rust/Cargo、Python 3，以及可显示中文的系统字体（例如 Noto Sans CJK）。

```sh
./install.sh
mchose-gui
```

应用菜单名称为 **迈从鼠标 / MCHOSE Mouse**。首次使用默认中文。右上角切换“简体中文 / English / Auto”，选择保存在 `~/.config/mchose/language`（遵守 `XDG_CONFIG_HOME`）。GUI 和 CLI 共用这个偏好；`--lang` 可以临时覆盖。

```sh
mchose --lang zh --help
mchose-gui --lang zh
./install.sh --desktop-only
```

最后一条只更新桌面入口和图标，不重建程序或调整系统设备规则。

## CS 与握持角度

“瞄准与角度”页面提供 DPI 档位、精确 DPI、回报率和传感器旋转。

- 传感器旋转支持 **−30°～+30°**、1° 步进，支持滑块、精确输入和归零。
- 传感器旋转调整坐标轴；**直线修正 / Angle Snapping** 拉直轨迹，两者独立。
- 滑块拖动时本地即时预览，松手后写入。精确输入使用“应用”按钮提交。
- 角度发送后必须读回一致才报告成功。设备不支持或未接受设置会显示错误。
- 内置 `cs` 和 `desk` 预设**保留当前角度**。自定义预设保存并恢复角度。

```sh
mchose rotation            # 查看当前角度
mchose rotation -8         # 设置为 -8°
mchose rotation 0          # 归零
mchose preset save 我的CS
mchose preset 我的CS
mchose preset delete 我的CS
```

2026-09-19 在 A7 Pro（接收器 USB 5253:1021，固件 5.4.7.4）实测了正负角度写入与读回，并恢复测试前设置。其他型号仍需分别验证。GUI 的角度图是坐标轴示意，不是自动测量握持角度。

## 界面与预设

- **瞄准与角度**：DPI、回报率、握持角度补偿。
- **传感器与响应**：运动同步、波纹控制、直线修正、LOD、消抖、休眠、性能模式。
- **预设管理**：保存当前设置、应用和删除自定义预设，支持中文名称。

已有的无 `rotation` 字段预设会保留当前角度。使用同名保存会覆盖原预设；删除内置名称的自定义覆盖项会恢复内置预设。

设备通信在后台串行执行；连续修改不同参数不会互相覆盖。失败时停止后续队列并显示错误，请刷新确认设备实际状态。GUI 和 CLI 使用设备文件锁避免彼此交叉发送报告。

## CLI 脚本接口

```sh
mchose --lang en show --json
mchose info --json
mchose dpi 800
mchose rate 1000
mchose backup mouse.bin
```

JSON 字段名和命令名不随界面语言改变。`rotation_degrees` 是有符号角度，DPI 档位编号为 0～5；GUI 用具体 DPI 值展示档位。

## 当前限制

- DPI、回报率和 LOD 的范围沿用已有协议实现，尚未加入完整的逐型号能力表。当前 DPI 范围为 50～26000，步进 50。
- LOD 无法读回，界面标明它是工具上次写入的缓存值。调整角度时同一传感器报告会携带此 LOD 值。
- 底层操作系统错误和原始通信日志保留原文，常用界面、帮助、输出与参数提示支持中英文。
- 自动切换基于前台应用，当前适配 KDE Plasma 6（Wayland / X11）；不按后台进程存活状态切换。

## 开发验证

```sh
cargo test --features gui
mchose-gui --demo --lang zh
mchose-gui --demo --lang en --page sensor
```

预览模式不会写设备或保存语言偏好。可配合 `--screenshot /绝对路径/preview.png` 生成 GUI 自身的截图。

## 按前台应用自动切换

打开 **应用自动切换** 页，搜索并选择应用，再选择预设，点击“保存绑定”后启用监听。普通绑定无需输入 app_id 或进程名；表达式收在默认折叠的高级设置中。
CS 在前台且命中规则时应用游戏预设；切到未匹配应用后恢复首次进入匹配状态之前的完整配置。
多个匹配预设连续切换时共用同一份原配置，不把上一个游戏预设当作恢复目标。

```sh
mchose auto bind cs 'app_id=steam_app_730 || class=steam_app_730 || exe=cs2'
mchose auto start
mchose auto status
mchose auto stop
```

如果要使用自定义 CS 角度，先设置角度并保存自己的预设，再绑定该预设：

```sh
mchose rotation -8
mchose preset save 我的CS
mchose auto bind 我的CS 'app_id=steam_app_730 || exe=cs2'
```

### 从已有应用选择

应用列表来自系统菜单，支持中文名称、搜索、用户菜单覆盖和已安装的 Steam 游戏。Steam 游戏按游戏标识绑定，不会把整个 Steam 客户端误认为某个游戏。

没有菜单入口的应用：启用监听，切到它再回来，点“最近使用”中的“选择此应用”即可。

CLI 也可以使用目录中的应用直接绑定：

```sh
mchose auto apps                   # 查看应用目录
mchose auto apps konsole           # 按名称搜索
mchose auto bind-app org.kde.konsole desk
```

### 高级过滤语法

| 字段 | 来源 | 示例 |
|---|---|---|
| `app_id` | KWin 的 desktopFileName，移除路径及 `.desktop` 后缀 | `app_id=org.kde.konsole` |
| `class` | KWin 的 resourceClass，兼容 XWayland | `class=steam_app_730` |
| `exe` | 前台窗口所属程序的可执行文件名 | `exe=cs2` |
| `path` | 前台程序的完整可执行路径 | `path=*/game/bin/linuxsteamrt64/cs2` |
| `title` | 窗口标题 | `title="*Counter-Strike*"` |

- `*` 匹配任意长度，`?` 匹配单个字符，匹配不区分大小写。
- `&&` 为同时满足，`||` 为任一满足，`!` 为取反。
- 优先级：`!` > `&&` > `||`，可使用括号。
- 含空格的值使用双引号；CLI 中请用单引号包住整条表达式，避免 shell 解释。
- 规则按列表从上到下匹配，首条启用且命中的规则优先；GUI 可上移、停用、移除。
- 这些是通配表达式，不是正则表达式，也不会执行 shell 命令。

```sh
mchose auto bind 我的CS '(app_id=steam_app_* || exe=cs2) && !title="*Launcher*"'
mchose auto rules
mchose auto disable 1
mchose auto enable 1
mchose auto remove 1
mchose auto inspect
mchose auto test 'exe=cs2'
```

每个桌面程序不一定提供 `app_id`。先启用监听，切到目标应用再回来，GUI 会保留最近外部应用的标识，可直接生成条件。没有 `app_id` 时使用 `class` 或 `exe`。

### 运行方式

- 后台监听独立于 GUI，关闭 GUI 不会停止；关闭“启用自动切换”或运行 `auto stop` 才停止，并恢复原配置。
- 焦点稳定 400 毫秒后才开始切换，减少快速切窗造成的重复写入；硬件写入还需要额外时间。
- 监听启动时不自动写鼠标，只有规则匹配发生变化时才写入。
- “登录时启动”可选，对应 `mchose auto autostart on` / `off`。默认不启用。
- 需要 Python 3、PyGObject（`gi`）及 KDE Plasma 6。KWin 脚本只读取窗口标识，停止时卸载。
- 配置文件：`~/.config/mchose/app-rules.conf`，遵守 `XDG_CONFIG_HOME`。状态与日志位于 `$XDG_RUNTIME_DIR/mchose/`。
- 未匹配、正常停止、监听桥接故障都会尝试恢复。若设备拔出导致恢复失败，界面报告错误；强制杀进程或断电无法保证即时恢复。

规则不会预先绑定或自动启用；请在界面保存自己的绑定后开启监听。


## 系统鼠标与桌面驻留（0.3）

「系统鼠标」页面直接读取 KDE / libinput 中这只迈从鼠标的设置：关闭加速（flat）、自适应加速，以及 −100～100 的系统速度刻度，0 为默认。该刻度不是百分比增益，也不是 Windows 的速度档位。DPI 和系统速度相互独立。系统只在发现唯一一只受支持的迈从指针设备时允许写入，不修改触摸板或其他品牌鼠标。当前后端面向 KDE Plasma；需要 Python 3 与 PyGObject（Gio），不支持的环境会显示错误，不会假装应用成功。

内置 `cs` 预设现在包含 **关闭系统加速 + 速度 0**，因此应用它也要求系统接口可用。旧预设不含系统字段时仍保留系统设置。保存新预设会包含可读取的系统速度与加速度。自动切换在进入匹配应用前记录系统和硬件设置，切出、暂停或正常退出时恢复；切换到未指定系统参数的另一条规则时沿用进入前的基线。原始输入游戏可能绕过系统指针设置，游戏内灵敏度单独调整。

```sh
mchose system show
mchose system speed 0
mchose system acceleration off
mchose system cs
```

「桌面与驻留」提供关闭窗口到托盘、登录自启到托盘，以及自动预设服务的登录自启。两种自启可独立开启。托盘支持打开设置、启用自动切换、暂停并恢复、退出并恢复。点击启动器会唤回已有窗口。托盘不可用时保持窗口可访问；不会隐藏一个无法唤回的窗口。默认不会自动替你开启登录自启。

```sh
mchose-gui --background
mchose-gui --page system
mchose-gui --page desktop
mchose desktop status
mchose desktop autostart on
mchose desktop close-to-tray on
```

正常退出会等待自动服务恢复完成。进程被强制杀死、断电或设备断开不保证完成恢复。系统设置通过 KDE 的设备配置接口写入，可能跨登录保留；测试与正常自动切换退出会显式恢复。

加速度定义参考：[libinput 官方说明](https://wayland.freedesktop.org/libinput/doc/latest/pointer-acceleration.html)。
