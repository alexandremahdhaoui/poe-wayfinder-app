# Smoke checking the overlay without the game

Six Windows syscalls cannot be tested from WSL. Five of them can be exercised
against a window that already exists, so only one needs Path of Exile running.

## Why a stand in window works

The window title is config. Point the overlay at `Program Manager`, which
every Windows desktop already has, and the whole window path runs: find it,
read its size, read its DPI, ask whether it is focused, and follow it every
frame.

Do not create a window for this. `Program Manager` is already there and
nothing on the user's desktop needs to change.

## The check

```sh
cd poe-trader-app && forge build poe-trader-windows
cd "$WIN_OUTPUT_PATH" && timeout 12 ./poe-trader.exe \
    --game poe2 --data-dir ./data --window-title "Program Manager"
```

Expected, one JSON line each:

| Line | Proves |
|---|---|
| `network policy` with `block_unlisted: true` | the allowlist is on before anything can dial out |
| `startup` with `item_names` and `stats` above zero | the data file loaded |
| `found the game window` with a size and scale | `FindWindow`, `GetWindowRect`, `GetDpiForWindow` |
| `foreground: false` | `GetForegroundWindow`, correctly reporting it is not focused |
| `registered the price check hotkey` | `RegisterHotKey` |

Running the full twelve seconds without exiting proves the frame loop runs and
follows the window.

A last observed run: `1707x1067` at scale `1.50`, which is a 2560x1600 display
at 150 percent. The scale being read rather than assumed is the point, because
the panel is positioned in scaled units and a wrong scale puts it off screen.

## What this does not cover

`SendInput`. It fires only on a hotkey press and types into whatever has
focus, so exercising it means either the game or typing into the user's
desktop. The keystroke order it sends is decided in
`poe_trader_core::controller::overlay::copy_key_sequence` and tested there;
what remains untested is the syscall itself.

**Do not simulate the hotkey to close this gap.** It sends Ctrl and C to
whatever is focused and overwrites the user's clipboard. That is their state,
not ours.

## The one check that needs the game

Launch Path of Exile, hover an item, press the price check key.

The overlay logs the outcome of every check with its cause, so a failure names
which link broke rather than leaving it to guesswork.
