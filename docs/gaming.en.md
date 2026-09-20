# CS2 gaming integration

[简体中文](gaming.md) | **English** · [Back to README](../README.en.md)

## Performance mode

CachyOS `game-performance` temporarily requests the performance power profile through power-profiles-daemon and restores the previous profile when the game exits. In Steam → CS2 → Properties → Launch Options:

```text
game-performance %command%
```

Preserve existing options and insert `game-performance` immediately before `%command%`. Do not stack it with `gamemoderun`. CachyOS warns that GameMode and ananicy-cpp can conflict over process priorities. This helper does not install or disable system scheduling services. [Official guidance](https://wiki.cachyos.org/configuration/gaming/)

Performance mode follows the game process lifetime. Mouse presets and Meta protection follow foreground focus and revert when you switch away. Lutris manages games and runtimes from multiple sources; Steam CS2 alone does not require migrating to it.

## Optional: suppress Meta desktop shortcuts while CS2 is focused

Requires KDE Plasma 6, KGlobalAccel, Python 3 / PyGObject (Gio) and user systemd. Verified on KDE 6.7.5 / Wayland. It runs independently of the mouse GUI, hardware and preset service.

From the repository root:

```sh
mkdir -p ~/.local/lib ~/.config/systemd/user
install -m 755 integrations/cs2-meta-guard.py ~/.local/lib/cs2-meta-guard.py
install -m 644 integrations/cs2-meta-guard.service ~/.config/systemd/user/cs2-meta-guard.service
systemctl --user daemon-reload
systemctl --user enable --now cs2-meta-guard.service
```

- KWin foreground events suspend standalone Meta and desktop keyboard shortcuts containing Meta. Alt+Tab, Shift+Alt+Tab and each action's non-Meta alternatives remain available.
- Switching away restores the original bindings, even if CS2 is still running in the background.
- This is not low-level keyboard remapping: it does not discard physical key events delivered to the game or handle Meta + mouse gestures.
- KDE's API updates shortcut configuration. Before changing bindings, the helper writes a recovery journal to `~/.local/state/cs2-meta-guard/pending-restore.json`. Normal shutdown, service cleanup and restart attempt restoration, preserving non-Meta edits made while gaming. Do not manually delete this journal while protection is active.
- Focus messages are accepted only from KWin. Application identifiers `steam_app_730` / `cs2` or a foreground executable named `cs2` trigger protection; window titles are not used.

Status, logs and disabling:

```sh
systemctl --user status cs2-meta-guard.service
journalctl --user -u cs2-meta-guard.service -n 30
systemctl --user disable --now cs2-meta-guard.service
```

If shortcuts remain disabled after a KDE restart or power loss, stop the service and restore the pending journal:

```sh
systemctl --user stop cs2-meta-guard.service
python3 ~/.local/lib/cs2-meta-guard.py --restore
```

`--self-test` briefly suppresses and restores Meta bindings to verify the API and Alt+Tab preservation. Run it only with the service stopped and outside gameplay. Restoration requires a working KDE session and cannot finish while power or D-Bus is unavailable.
