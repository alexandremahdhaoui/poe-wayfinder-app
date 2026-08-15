# experience: the controller player

**Recommendation: use Steam Input. We build nothing.**

This document is mostly about setting that up well. The build option is at the
end, because the research in `scope.md` says it is worse on every axis that
matters to the player.

## Where a controller player stands today

| What they want | Works today | Why |
|---|---|---|
| The game copies the item they are inspecting | **Yes** | The game does not need a cursor. Right stick inspect then a copy keystroke works |
| The overlay parses that item | **Yes** | The parser reads clipboard text. It never knew where the item came from |
| The panel shows prices | **Yes** | Nothing in the price check touches a controller |
| Fire the check without a keyboard | **No** | The overlay listens for Ctrl+D on a keyboard hook. A gamepad button is not a keyboard event |
| The panel opens where they can see it | **No** | The panel anchors to the mouse cursor, which has not moved for an hour |
| The panel closes when they are done | **No** | It closes when the mouse moves more than 38 points. The mouse never moves |
| Roll ranges appear on the stat rows | **No** | The overlay sends Ctrl+C only. It never holds the game's advanced item descriptions key |
| The in game price check | **Yes**, in town and hideout only | GGG shipped it in patch 0.5. It does not work while mapping, which is when players want it |

**One thing is broken and it is the trigger.** Everything downstream already
works. That is why a page of setup instructions can deliver this feature.

## Flow 1: setting it up, once

1. The player opens Steam.
2. If they play Path of Exile 2 through Steam, they skip to step 5.
3. If they play the standalone client, they click `Add a Game` at the bottom
   left, then `Add a Non-Steam Game`, then browse to `PathOfExile.exe` or
   `PathOfExileSteam.exe` and add it.
4. From now on they launch the game from Steam. Steam Input only applies to
   games Steam launched.
5. They right click Path of Exile 2 in the library, then `Manage`, then
   `Controller Layout`.
6. They pick a button the game does not use. On a fresh PoE2 controller layout
   the safest are the dpad Down long press, or a chord of two shoulder buttons,
   or Back plus a face button.
7. They add a command on that button. Steam calls it `Add Command`. They type
   the key combination `Ctrl+D`.
8. They save the layout under a name they will recognise later.
9. They start the game from Steam. They start the overlay.

**What they see when it works:** they press the button they chose, the price
panel appears, and their character does not move.

**What they see when it does not:** the character does something. That means
the game also acted on that button. Go back to step 6 and pick a different one.

## Flow 2: pricing an item in a map

1. The player finds a rare item on the ground and picks it up.
2. They open their inventory with the controller.
3. They highlight the item with the right stick and hold to inspect it, the same
   way they read any item.
4. They press their chosen button.
5. Steam Input sends Ctrl+D. The overlay's keyboard hook sees it and eats it, so
   the game never receives it.
6. The overlay sends the copy keystroke to the game window. The game copies the
   inspected item.
7. The overlay parses the item, builds a trade query and searches.
8. The panel appears with the price and the stat rows.
9. The player reads the price.
10. They press the same button again, or press Escape through Steam Input, to
    close the panel.

**Time budget:** the panel is up within 1200ms of the press. `press-check.sh`
already asserts that on the `elapsed_ms` field. Nothing about a controller
changes it.

## Flow 3: the panel appears in the wrong place

This is the flow we must fix in code even under the Steam Input plan.

1. The player triggers a check with the controller.
2. The panel opens at the last place the mouse cursor sat, which may be another
   monitor.
3. The player cannot reach it, because moving the mouse means putting down the
   controller.

**The fix is one line of wiring, not a feature.** `overlay_lifecycle` already
has `begin_locked`, the path the locked hotkey uses. It opens the panel focused
with no pointer involved and it does not close on mouse movement. A check
triggered while the game reports controller input should take that path.

Until that lands, the player's workaround is to leave the mouse in the middle of
the game window before they pick up the controller.

## Acceptance criteria

Each of these is measurable and each of them fails today.

| # | Criterion | How it is measured |
|---|---|---|
| A1 | A controller button opens the price panel with no keyboard touched | The player presses it. The `price check finished` log line appears |
| A2 | The character takes no action when that button is pressed | Watch the character. Nothing moves, nothing casts, no flask fires |
| A3 | The panel is inside the game window rect | Compare the panel rect on the log line against the game window rect |
| A4 | The panel is visible within 1200ms of the press | The `elapsed_ms` field, already asserted by `press-check.sh` |
| A5 | The panel closes on a controller input and does not close on its own | The panel stays up for at least 30 seconds with nobody touching the mouse |
| A6 | The stat rows carry roll ranges | The `stat_rows` field on `price check finished` is above zero and the rows show bounds |
| A7 | Nothing new leaves the machine | The allowlist in `http_adapter.rs` is unchanged |
| A8 | Setup takes under 5 minutes for someone who has never opened a Steam controller layout | Watch one person do it from Flow 1 with no help |

A6 is the advanced descriptions bug. It is not caused by the controller. It is
caused by `clipboard_adapter::trigger_copy` sending Ctrl and C and dropping
every other key. It shows up worse on controller because the controller tooltip
is the richer one.

## Failure modes and what the player sees

| Failure | What the player sees now | What they should see |
|---|---|---|
| No controller connected | Nothing. The overlay never knew about controllers | Nothing. This is correct. The keyboard hotkey still works |
| Controller disconnects mid session | The button stops working with no message | A tray notice reading `controller disconnected`. Never a modal. Never anything that steals focus during a boss fight |
| The button is one the game also acts on | The panel opens **and** the character moves or casts | The setup page must warn about this in bold, because we cannot prevent it. There is no gamepad hook on Windows and no way to consume a button. Pick a dead combination |
| A DualSense the system reports as something else | The button does nothing, or Steam Input reports a generic gamepad | The setup page names the fix: enable `PlayStation Controller Support` in Steam settings under Controller. If they are not on Steam, they need DS4Windows or DS5Windows, which presents the pad to Windows as an Xbox controller |
| Steam Input is on and the player also enabled our XInput reading | Neither works reliably. Steam Input takes exclusive control of the pad and blocks XInput and DirectInput for everyone else | If we ever ship the adapter it must detect Steam Input and refuse, with a log line saying why |
| The item was not inspected before the press | The panel says no item text found | Same message. The player learns to inspect first. This matches keyboard behaviour, where the item must be under the cursor |
| The player is in an area where the game refuses to copy | No item text found | Known game behaviour. Divination card tabs and a few other panels do not answer a copy at all |
| Another program stole Ctrl+Alt+C | Nothing happens, no error | Log `No item text found`. The known thieves are Discord clips, ASUS GPU Tweak, Radeon Software and Display Pilot. Name them on the setup page |

## Why we do not build gamepad reading

Stated plainly so nobody relitigates it from the summary.

1. **We cannot swallow the button.** Windows has fifteen hook types and none is
   a gamepad hook. XInput has seven functions and every one reads state or sets
   rumble. There is no callback, no filter, no handled flag. The only thing that
   blocks a pad from a game is a kernel driver, which we will not ship. So every
   binding we offer would also reach the game, and PoE2 uses nearly every button.
   Steam Input owns the mapping instead, so an unbound button sends nothing.
2. **A DualSense is not an XInput device.** It is a DirectInput HID pad. It
   reaches XInput only through Steam Input, DS4Windows or ViGEm. Every player
   who solved that already installed the thing that solves the trigger too.
3. **The two approaches cancel out.** When Steam Input is active it takes the
   pad exclusively and our reads return nothing. We would build a feature that
   dies the moment the player picks the better one.
4. **It works today.** Steam Input needs no build, no deploy, no Smart App
   Control retry loop and no new code to maintain.

## What we should build

Two small things. Neither is gamepad reading.

| Change | Why | Size |
|---|---|---|
| A gamepad triggered check opens the panel through `begin_locked` | Fixes Flow 3. The panel is unreachable otherwise | Small. The path already exists |
| Hold the advanced item descriptions key during the copy | Fixes A6 for every player. `keys_to_hold_for_copy` is already ported, already tested and called by nothing | Small. Wire an existing pure function into `trigger_copy` |

Both belong in their own scope documents. Neither needs a controller to be
worth doing.

## Open question for the user

Do we write the Steam Input setup page and stop, or do we build the XInput
adapter knowing it can never stop the game seeing the button and that it stops
working the moment the player turns on Steam Input?
