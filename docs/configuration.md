# Configuration

Woop reads its settings from `config.toml`:

| Platform | Location |
| --- | --- |
| Windows | `%APPDATA%\woop\config.toml` |
| macOS and Linux | `$XDG_CONFIG_HOME/woop/config.toml`, or `~/.config/woop/config.toml` |

The file is optional; Woop never creates or changes it. Changes apply while Woop runs, within about two seconds: colours and the font at once in every pane, the shell in panes opened afterwards. If the file has a mistake, Woop keeps the settings it was using and shows the error at the top of the window until the file is fixed.

Every setting is optional. A complete example:

```toml
[font]
family = "Cascadia Mono"   # default: Consolas on Windows, Menlo on macOS, the system monospace font elsewhere
size = 14                  # 6 to 72

[shell]
program = "pwsh.exe"       # default: your usual shell
args = ["-NoLogo"]

[colors]
theme = "dark"             # "dark" (default) or "light"
# Any of these replace the theme's colour, written as #rrggbb.
foreground = "#d4d7dd"
background = "#16181d"
cursor = "#7aa2f7"
selection = "#2f3d58"
# The 16 ANSI colours: black, red, green, yellow, blue, magenta, cyan,
# white, then their bright variants in the same order.
ansi = [
  "#1f2229", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#c8ccd4",
  "#5c6370", "#ff7a85", "#b5e890", "#ffd68a", "#82c4ff", "#de9bf2", "#7fd3de", "#f5f6f8",
]
```

Unknown settings are reported as mistakes, so a misspelt name does not go unnoticed.

On macOS and Linux, Woop ignores the file if other users could change it or the directory it is in, since it chooses the program Woop starts (`chmod 600` on the file fixes it). The shell program must be an absolute path or a name found in `PATH`.

The decisions behind this are in [ADR 0020](adr/0020-toml-configuration.md).
