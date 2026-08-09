# PoE Wayfinder

Overlay price checker for Path of Exile 1 and 2 in a single Rust binary.

## What it does

- Prices the item under your cursor in both PoE1 and PoE2 from one executable.
- Detects which game is running from its window and follows you when you alt-tab
  between them.
- Turns the item into an editable filter panel: stat rolls with ranges, item
  properties, flags like corrupted and mirrored.
- Sockets runes and soul cores into the item before searching, so you can price
  the item you are about to make (PoE2).
- Estimates a price from the listings the trade site returned, computed on your
  machine.

## How to use it

1. Start Path of Exile or Path of Exile 2 in borderless windowed. Exclusive
   fullscreen blocks any overlay.
2. Run PoE Wayfinder. A status window opens showing the detected game, hotkey,
   league and loaded data.
3. Press **Hide to tray** to get it out of the way. The tray icon brings it back.
4. Hover an item in game and press **Ctrl+D**.
5. Adjust the filters, then search again or open the result in your browser.

Hotkeys: **Ctrl+D** price check. **Escape** or clicking away closes the panel.
Holding **Alt** hides the overlay while pressed, because Alt is the game's
show-modifiers key.

## Why it is different

Awakened PoE Trade covers PoE1 and Exiled Exchange 2 is a PoE2 fork of it, so
running both games means running two apps. Wayfinder ports both feature sets
into one Rust binary that switches games at runtime. Its game data is compiled
into the executable, so there is nothing to install beside it and nothing to
download on first run. It contacts `www.pathofexile.com` and nothing else,
enforced by a host allowlist in a single network layer; price estimation is
computed from the trade API's own listings rather than sent to a third-party
prediction service. It reads the clipboard and window titles only, and does not
touch game memory.

## Requirements and install

- Windows 10 or 11.
- Path of Exile or Path of Exile 2 in borderless windowed mode.
- Download the executable and run it. No installer, no data folder, no session
  token required.
- Optional overrides: `--game`, `--league`, `--data-dir`.
- The build is unsigned, so Smart App Control may block it.
- Discord Overlay and NVIDIA GeForce Overlay compete for always-on-top; disable
  them if the panel sits behind the game.

## Licence

Apache License 2.0, see `LICENSE`.

This is a port of Awakened PoE Trade and Exiled Exchange 2, both MIT licensed.
Their notice and a statement of what was taken and what changed are in `NOTICE`.

Game data belongs to Grinding Gear Games and is not covered by either licence.
See `../poe-wayfinder-data/README.md`. This project is not affiliated with or
endorsed by Grinding Gear Games.
