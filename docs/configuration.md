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

## Shortcuts

The `[keys]` table gives an action a new shortcut, or removes it with `"none"`. The command palette (Ctrl+Shift+P, Cmd+Shift+P on macOS) shows each action's shortcut in effect.

```toml
[keys]
focus-left = "Ctrl+Alt+Left"         # instead of Alt+Left
focus-right = "Ctrl+Alt+Right"
copy-last-command = "Ctrl+Shift+Y"   # a command that has no shortcut by default
split-down = "none"                  # no shortcut
```

Actions: `new-tab`, `close-pane`, `next-tab`, `previous-tab`, `split-right`, `split-down`, `focus-left`, `focus-right`, `focus-up`, `focus-down`, `command-palette`, `copy-last-command`, `copy-last-output`, `toggle-last-output`, `previous-block`, `next-block`, `copy-selection`, `paste`, `scroll-to-bottom`, `copy-config-path`, `reload-config`.

A shortcut is written as modifiers and a key joined by `+`: modifiers `Ctrl`, `Alt` (or `Option`), `Shift`, `Cmd` (or `Super`, `Win`); keys a character, `F1` to `F12`, or `Up`, `Down`, `Left`, `Right`, `Tab`, `Enter`, `Space`, `Escape`, `Backspace`, `Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`. A shortcut must include Ctrl, Alt or Cmd, so that it cannot take a key used for typing, and Ctrl+Alt with a character is refused because it types characters on many keyboards (AltGr). Giving an action a shortcut that another action had moves it to the new action; giving two actions the same shortcut in `[keys]` is a mistake. The platform's copy and paste keys always work; `copy-selection` and `paste` add more.

Some shortcuts take keys from the shell: Ctrl+C interrupts a command, Ctrl+D ends input, Ctrl+Z suspends, Ctrl+R searches the history. Woop allows them, but the shell no longer receives them. On macOS, Option with a character types characters (å, @), so such shortcuts are ignored there. A shortcut with a symbol that needs Shift on your keyboard is written with the symbol Shift produces (`Ctrl+Shift+!`, not `Ctrl+Shift+1`).

On macOS and Linux, Woop ignores the file if other users could change it or the directory it is in, since it chooses the program Woop starts (`chmod 600` on the file fixes it). The shell program must be an absolute path or a name found in `PATH`; if it cannot start, the pane opens your default shell and says why.

Command blocks and the command editor need Woop's shell integration, which loads only with shell arguments known not to keep it from running: `-NoLogo` and `-NoProfile` for PowerShell, `-i` for bash, `-i`, `-l` and `--login` for zsh and fish. With any other argument (a login bash, `-Command`, `-c`), the shell starts without it, as in a plain terminal.

The decisions behind this are in [ADR 0020](adr/0020-toml-configuration.md).
