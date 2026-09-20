# CS2 游戏集成

**简体中文** | [English](gaming.en.md) · [返回 README](../README.md)

## 性能模式

CachyOS 的 `game-performance` 会通过 power-profiles-daemon 暂时启用性能配置，游戏退出后恢复原配置。Steam → CS2 → 属性 → 启动选项：

```text
game-performance %command%
```

如果已有启动参数，请保留它们，将 `game-performance` 放到 `%command%` 前面。不要同时叠加 `gamemoderun`。CachyOS 官方提醒 GameMode 与 ananicy-cpp 的进程优先级设置可能冲突；本工具不会安装或停用系统调度服务。[官方说明](https://wiki.cachyos.org/configuration/gaming/)

性能模式跟随游戏进程生命周期；鼠标预设和下述 Meta 保护跟随前台窗口，切出即可恢复。它们是不同的设置。Lutris 用于管理多个来源的游戏及运行环境，仅为了 Steam CS2 不必迁移到它。

## 可选：CS2 前台屏蔽 Meta 桌面快捷键

需要 KDE Plasma 6、KGlobalAccel、Python 3 / PyGObject（Gio）和用户 systemd。在 KDE 6.7.5 / Wayland 上验证。安装后独立于鼠标 GUI、鼠标设备和自动预设服务运行。

在仓库根目录执行：

```sh
mkdir -p ~/.local/lib ~/.config/systemd/user
install -m 755 integrations/cs2-meta-guard.py ~/.local/lib/cs2-meta-guard.py
install -m 644 integrations/cs2-meta-guard.service ~/.config/systemd/user/cs2-meta-guard.service
systemctl --user daemon-reload
systemctl --user enable --now cs2-meta-guard.service
```

- KWin 报告 CS2 前台焦点时，暂停单独 Meta 及包含 Meta 的桌面键盘快捷键；保留 Alt+Tab、Shift+Alt+Tab 和各动作的非 Meta 绑定。
- 切出 CS2 后恢复原绑定；游戏留在后台时不会继续屏蔽。
- 这不是底层键盘重映射：不会吞掉发给游戏的物理按键，也不处理 Meta + 鼠标手势。
- KDE 接口会更新快捷键配置，因此修改前将待恢复绑定记入 `~/.local/state/cs2-meta-guard/pending-restore.json`。正常停止、服务退出清理和重启都会尝试恢复；保留游戏期间修改的非 Meta 绑定。不要在服务生效期间手动删除此恢复文件。
- 只接受 KWin 的焦点消息。应用标识 `steam_app_730` / `cs2` 或实际前台进程 `cs2` 会触发保护，不按窗口标题匹配。

查看状态、日志和停用：

```sh
systemctl --user status cs2-meta-guard.service
journalctl --user -u cs2-meta-guard.service -n 30
systemctl --user disable --now cs2-meta-guard.service
```

若 KDE 服务重启或断电后仍有快捷键未恢复，先停止保护服务，再执行恢复：

```sh
systemctl --user stop cs2-meta-guard.service
python3 ~/.local/lib/cs2-meta-guard.py --restore
```

`--self-test` 会短暂暂停 Meta 绑定并立即恢复，用于检查接口及 Alt+Tab 保留；仅在保护服务停止、没有游戏对局时运行。恢复依赖可用的 KDE 会话，不保证在断电或 D-Bus 不可用的瞬间完成。
