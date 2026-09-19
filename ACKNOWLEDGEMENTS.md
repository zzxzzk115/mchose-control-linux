# 致谢 / Acknowledgements

[简体中文](README.md) · [English](README.en.md)

## 上游 / Upstream

This project derives from **[alexfrih/mchose-linux](https://github.com/alexfrih/mchose-linux)** by **Alexandre Frih**, upstream commit `54e0c7d`. Its protocol reverse engineering, Rust HID implementation, original CLI/GUI, protocol documentation and analysis tools are the foundation of this repository. The original MIT copyright notice is retained in [LICENSE](LICENSE). This repository starts with one independent initial commit; attribution to the original work is retained here.

本项目在上述基础上增加了中英文界面、角度可视化、串行设备操作、系统鼠标设置、前台应用预设切换、应用选择、托盘及桌面启动集成。原项目的代码和协议工作应归功于原作者。

## Components / 组件

| Component | Role | License / licensing reference |
| --- | --- | --- |
| [egui / eframe](https://github.com/emilk/egui) | Native GUI and rendering stack | MIT OR Apache-2.0; see dependency inventory |
| [libc](https://github.com/rust-lang/libc) | Linux system interfaces | MIT OR Apache-2.0 |
| [KWin](https://invent.kde.org/plasma/kwin) | Foreground windows and Wayland lifecycle | External desktop service; see upstream LICENSES |
| [libinput](https://gitlab.freedesktop.org/libinput/libinput) | Pointer speed and acceleration through KDE | External runtime component; see upstream COPYING |
| [PyGObject](https://gitlab.gnome.org/GNOME/pygobject) | Python bindings for Gio D-Bus | External dependency; see upstream COPYING |
| [GLib / Gio](https://gitlab.gnome.org/GNOME/glib) | D-Bus integration | External dependency; see upstream COPYING |
| [Babel](https://github.com/babel/babel) | Parser, traversal and generator in protocol tools | MIT; see upstream LICENSE |
| [Noto CJK](https://github.com/notofonts/noto-cjk) | Recommended system font for Chinese | SIL Open Font License 1.1; loaded from the system, not bundled |

The complete Linux GUI Cargo dependency inventory, including transitive crates and their declared license expressions, is in [THIRD_PARTY.md](THIRD_PARTY.md). `Cargo.lock` also includes packages for other target platforms. Dependencies are fetched by build/package tools; their source trees are not vendored in this repository. Bundled fonts and other assets within dependencies remain covered by those dependencies' own notices. This document does not replace any upstream license.

MCHOSE's M HUB JavaScript bundle remains vendor-owned and is excluded from this repository. The inherited `protocol/` scripts fetch it separately. Product and application names in screenshots identify interoperability targets and do not imply endorsement.
