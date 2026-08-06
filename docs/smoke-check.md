# Smoke checking the overlay without the game

Six Windows syscalls cannot be tested from WSL. All six are exercised against
a window that already exists, so none of them needs Path of Exile running.

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
| `keyboard input works` with `events_accepted: 2` | `SendInput` |
| `registered the price check hotkey` | `RegisterHotKey` |

Running the full twelve seconds without exiting proves the frame loop runs and
follows the window.

A last observed run: `1707x1067` at scale `1.50`, which is a 2560x1600 display
at 150 percent. The scale being read rather than assumed is the point, because
the panel is positioned in scaled units and a wrong scale puts it off screen.

## How SendInput is covered without touching the clipboard

It used to be the one call nothing reached. It fires only on a hotkey press
and types into whatever has focus, so exercising it the ordinary way means
sending Ctrl and C to the user's desktop and overwriting their clipboard.

`self_test_send_input` sends `VK_NONAME` instead. Windows documents it as
reserved: no character, no command, no binding anywhere. Same struct, same
size argument, same up and down pair, same return check as a real copy. Only
the key differs.

It runs at startup and logs the result, because a build whose SendInput does
not work cannot copy an item and should say so before the user presses the
hotkey and sees nothing happen.

**Do not simulate the hotkey instead.** That sends Ctrl and C to whatever is
focused and overwrites the user's clipboard. That is their state, not ours.

## Both games

The data directory and the window title are the only differences.

```sh
./poe-trader.exe --game poe1 --data-dir ./data-poe1 \
    --window-title "Program Manager" --price-check-hotkey "Ctrl+Q"
```

PoE1 reports `game configuration: not found` on a machine with only PoE2
installed. That is the right answer, and the line exists so a setup problem is
visible at startup rather than in quietly wrong prices.

**Kill the previous run first.** A still-running instance holds the hotkey and
the next one fails to register it, which reads like a bug in the new build.

## The one check that needs the game

Launch Path of Exile, hover an item, press the price check key. This is the
only thing left that a stand in window cannot prove: that the game itself
responds to the keystroke by writing the item to the clipboard.

The overlay logs the outcome of every check with its cause, so a failure names
which link broke rather than leaving it to guesswork.
