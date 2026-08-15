# scope: controller support

Research only. No code was written. No dependency was added.

## The headline

**Item copying works in controller mode. The feature is alive.**

Path of Exile 2 copies the item the player is inspecting with the right stick
when the copy keystroke arrives. It does not need a mouse cursor and it does not
need a mouse click. The whole price check contract survives.

The broken part is the trigger, not the copy. A controller player can already
run our full price check today. They just have to reach for the keyboard to
press Ctrl+D.

**Gamepad input cannot be swallowed.** There is no gamepad equivalent of
`WH_KEYBOARD_LL`. Windows offers no hook, no filter and no consume flag for a
gamepad. Whatever button we read, the game reads too, in the same instant.

**Recommendation: ship Steam Input as documentation first. Build nothing yet.**
Steam Input already solves the trigger and it solves the swallow problem we
cannot solve ourselves. Building an XInput adapter costs real work and delivers
a worse trigger, because ours cannot suppress the button.

## Problem

A controller player holding an Xbox pad or a DualSense cannot fire the overlay
without a keyboard. The overlay listens for Ctrl+D on a low level keyboard hook.
A gamepad button produces no keyboard event, so nothing fires.

Two smaller problems follow from that.

1. `overlay_lifecycle` anchors the panel to the mouse cursor and closes it when
   the pointer moves more than 38 logical points. In controller mode the cursor
   never moves. The panel would open wherever the mouse was left, possibly on
   another monitor, and would never close on its own.
2. `KeyboardCopyTrigger::trigger_copy` sends Ctrl and C only. It never holds the
   game's advanced item descriptions key. This is a live bug for every player
   and it costs a controller player more, because their tooltips carry the
   detail our parser wants.

## The hard question, answered

### 1. Does the game copy an item when the player uses a controller

Yes. It copies the item the controller is inspecting, not an item under a
hidden cursor. Exiled Exchange 2 has been price checking on controller for
whole leagues. Its maintainer describes the copy path working on controller and
describes one specific regression, which is the next answer.

Source: https://github.com/Kvan7/Exiled-Exchange-2/discussions/478

### 2. Is there a native binding for copy item to clipboard

There is no gamepad binding for copy. There is a keyboard binding.

- Ctrl+C copies the plain item text.
- The game exposes `show_advanced_item_descriptions` in its own config ini
  under `[ACTION_KEYS]`. Holding that key while pressing Ctrl+C copies the
  advanced text with the roll ranges. Alt is the default, so the combination is
  usually Ctrl+Alt+C.
- Exiled Exchange 2 documents Ctrl+Alt+C as the copy it sends, and its
  troubleshooting page is entirely about that combination being stolen by other
  software.

Source: https://kvan7.github.io/Exiled-Exchange-2/nothing-happens.html

Both references read the key out of the game config rather than hardcoding it.
`reference/Exiled-Exchange-2/main/src/host-files/GameConfig.ts:88` and
`reference/Exiled-Exchange-2/main/src/shortcuts/Shortcuts.ts:293`.

We already port that read. `core::controller::game_config::show_mods_key` and
`core::controller::overlay::keys_to_hold_for_copy` both exist and both pass
tests. **Nothing in `poe-wayfinder-app` calls either of them.**
`clipboard_adapter::trigger_copy` maps only `"Ctrl"` and `"C"` and silently
drops every other key with `_ => return None`. That is the pre existing bug
named above.

The 0.7.1 regression in Exiled Exchange 2 was exactly this. The advanced copy
was wired for keyboard and mouse and not for controller, so uniques came back
without affixes. It is a wiring bug in the tool, not a game limitation.

### 3. Does moving the mouse break the controller session

**Unresolved. This is the one thing to verify in game before writing code.**

Sources disagree. Older reports say the input mode is a settings choice and
changing it needs a logout. Newer reports say the game detects the active device
and switches. GGG discussed seamless switching as planned work.

The risk this creates is small either way. We never move the mouse. Nothing in
the price check path calls `SetCursorPos`. `overlay_lifecycle` only reads the
pointer. So the overlay does not itself flip the input mode. What matters is
whether a *player* who nudges the mouse loses their controller bindings, which
is a game behaviour we cannot change and only need to know about.

Sources:
- https://www.pathofexile.com/forum/view-thread/3901619
- https://www.mmorpg.com/editorials/80-hours-in-and-playing-path-of-exile-2-on-controller-may-not-be-the-right-call-2000133920

### 4. Where does the panel go

Not at the cursor.

`core::controller::overlay_lifecycle::CLOSE_THRESHOLD` is 38.0 and `begin`
stores the pointer as the anchor. With a stationary mouse the panel opens at a
stale point and the close rule can never fire.

The fix already exists in the tree. `begin_locked` is the path the locked hotkey
uses. It opens the panel focused with no mouse involved, and the workspace
CLAUDE.md already names it as the deterministic way to open a panel without a
pointer. **A gamepad triggered check should use the locked path.** The panel
then needs a gamepad button to close it, because Escape is also a keyboard key.

## The input side

### 5. The Windows APIs

| API | Covers Xbox | Covers DualSense | Works unfocused | Verdict |
|---|---|---|---|---|
| XInput | Yes, native | Only through DS4Windows or Steam or ViGEm | Yes | **The only viable one** |
| Windows.Gaming.Input | Yes | Partially | **No.** Needs an in focus window in the process | Rejected |
| Raw HID | Yes | Yes, natively | Yes | Rejected. Per device parsing, per firmware breakage |
| DirectInput | Yes, as a generic pad | Yes | Yes | Rejected. Deprecated, loses the trigger axes |

The focus rule kills Windows.Gaming.Input outright. Our overlay is never the
foreground window while the player plays. The game is. gilrs documents the same
constraint for its default backend: "Windows Gaming Input requires an in focus
window to be associated with the process to receive events".

A DualSense is not an XInput device. Windows enumerates it as a DirectInput HID
gamepad. It reaches XInput only when DS4Windows, DS5Windows or Steam Input
presents a virtual Xbox pad through ViGEmBus. So an XInput adapter serves
DualSense users only when they already run one of those, and if they run Steam
Input then Steam Input already solves their problem and our adapter is dead
weight.

Sources:
- https://learn.microsoft.com/en-us/windows/win32/xinput/getting-started-with-xinput
- https://docs.rs/gilrs/latest/gilrs/
- https://ds4-windows.com/

### 6. Rust crates

| Option | Licence | Maintained | Needs a window or pump | Background thread | Verdict |
|---|---|---|---|---|---|
| `windows` 0.62, feature `Win32_UI_Input_XboxController` | MIT OR Apache-2.0 | Yes, Microsoft | No | Yes | **Use this** |
| `windows` 0.62, feature `Gaming_Input` | MIT OR Apache-2.0 | Yes, Microsoft | Yes, needs focus | Yes | Rejected on focus |
| `gilrs` 0.11.2 | Apache-2.0 OR MIT | Yes, May 2026 | Default backend needs focus | Not documented | Rejected |
| `sdl2` | Zlib | Yes | Ships SDL2.dll beside the exe | Yes | Rejected |

**No new dependency is needed.** `poe-wayfinder-app/Cargo.toml` already depends
on `windows = "0.62"` behind `cfg(windows)`. The XInput bindings live in that
same crate behind one feature string. Verified in the vendored source at
`~/.cargo/registry/.../windows-0.62.2/Cargo.toml:696` and
`src/Windows/Win32/UI/Input/XboxController/mod.rs`. Enabling a feature on a
crate we already build adds zero transitive crates.

`sdl2` is rejected on a workspace rule, not on quality. Workspace CLAUDE.md
says `poe-wayfinder.exe` takes no arguments and runs with nothing beside it.
A required DLL breaks that.

### 7. Can gamepad input be swallowed

**No. Confirmed, not a guess.**

`SetWindowsHookExW` accepts exactly fifteen hook types. Every one of them is
listed in the Microsoft reference: `WH_CALLWNDPROC`, `WH_CALLWNDPROCRET`,
`WH_CBT`, `WH_DEBUG`, `WH_FOREGROUNDIDLE`, `WH_GETMESSAGE`,
`WH_JOURNALPLAYBACK`, `WH_JOURNALRECORD`, `WH_KEYBOARD`, `WH_KEYBOARD_LL`,
`WH_MOUSE`, `WH_MOUSE_LL`, `WH_MSGFILTER`, `WH_SHELL`, `WH_SYSMSGFILTER`.
None of them is a gamepad hook. There is no `WH_GAMEPAD`.

XInput exports exactly seven functions. I read the list straight out of the
`windows` crate: `XInputEnable`, `XInputGetAudioDeviceIds`,
`XInputGetBatteryInformation`, `XInputGetCapabilities`, `XInputGetKeystroke`,
`XInputGetState`, `XInputSetState`. Every one either reads state or sets rumble.
There is no callback to register and no value to return that means handled.

The only way to stop a game seeing a gamepad on Windows is a kernel mode filter
driver. HidHide is that driver. It hides a whole device from a whole application
and requires an install and a reboot. It is all or nothing per device. It cannot
suppress one button press and pass the next.

**So every gamepad binding we build must use a combination the game does not
act on.** That constraint shapes the whole design, and it is the reason Steam
Input wins, because Steam Input sits above the game's view of the pad and is not
subject to it.

Sources:
- https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw
- https://github.com/nefarius/HidHide

### 8. Polling cost

Microsoft states it plainly: "For performance reasons, don't call
`XInputGetState` for an 'empty' user slot every frame. We recommend that you
space out checks for new controllers every few seconds instead."

Connected slots are cheap. Empty slots are not.

Our frame loop runs at `FRAME_INTERVAL = 100ms`, set in
`src/driver/overlay_loop/win.rs:47`. Polling all four slots every frame is 40
calls a second, and up to 40 of those hit empty slots.

The design that respects the guidance:

- Poll only slots already known connected, once per frame. At most 10 calls a
  second per pad.
- Rescan for new pads every 5 seconds.
- Drop a slot back to unknown when `XInputGetState` returns
  `ERROR_DEVICE_NOT_CONNECTED`.

That fits inside the existing frame loop. **No separate thread is needed.**
`XInputGetState` on a connected pad is a memory read against a driver buffer
and does not block. Adding a thread would buy nothing and would cost a channel,
a shutdown path and a test seam.

Source: https://learn.microsoft.com/en-us/windows/win32/xinput/getting-started-with-xinput

### 9. Steam Input, the alternative that needs no code

Steam Input maps any controller input to any keyboard output, for any game in
the Steam library, including a non Steam shortcut. Path of Exile 2 is itself a
Steam game, so a Steam player needs no shortcut at all. A standalone client
player adds it once with Add a Non Steam Game.

Why it beats building this ourselves:

| | Steam Input | An XInput adapter we build |
|---|---|---|
| Can suppress the button | Yes. Steam owns the mapping and an unbound button sends nothing | **No. Never.** |
| Covers DualSense | Yes, natively, USB and Bluetooth | Only via DS4Windows or ViGEm |
| Covers Bluetooth Xbox pads | Yes | Yes |
| Code we write | None | An adapter, a core controller, wiring, tests |
| Code we maintain | None | All of it, forever |
| Works today | Yes | No |
| Extra install | Steam, which most players already run | None |

Our low level keyboard hook sees injected input. Workspace CLAUDE.md states
that as a measured fact, and `hack/press-check.sh` depends on it. Steam Input
emits its keyboard output through the same synthetic input path. So a Steam
Input chord mapped to Ctrl+D should fire our existing hotkey with no change to
this codebase at all.

That is the whole feature, delivered, for the price of a page of setup
instructions.

Sources:
- https://steamcommunity.com/sharedfiles/filedetails/?id=3462829061
- https://www.rewasd.com/blog/post/how-to-remap-any-controller-on-pc-complete-guide

### 10. GGG's rules

Reading a controller and sending one keystroke does not change the answer. It is
still one action per keypress and there is still no timer.

GGG's position, as recorded in the community rules thread that GGG staff have
repeatedly endorsed:

- One key press equals one server action.
- "A macro must be something you activate manually by interacting with your
  mouse or keyboard." No timers. No loops. No conditions.
- The method does not matter. AutoHotkey, mouse software, a physical popsicle
  stick, all judged the same. GGG cares what the player does, not how.
- Moving the cursor and then clicking is called out as botting.

Our price check triggers no server action at all. It copies text and queries the
trade API, which is a separate rate limited service and already the whole point
of the app. Adding a controller as the thing that starts it changes nothing.

GGG publishes no rule specific to controller remapping. Every mainstream
remapper, Steam Input included, sits in the same category as mouse software,
which is explicitly fine.

Source: https://www.pathofexile.com/forum/view-thread/2077975

## Goals

1. A controller player fires the price check without touching a keyboard.
2. The panel opens somewhere they can see and closes on a controller input.
3. The advanced item descriptions key is held during the copy, so a controller
   tooltip parses with its roll ranges.
4. Zero new outbound hosts. Zero new crates.
5. Every binding uses an input the game does not act on, because we cannot
   swallow it.

## Non goals

1. Driving the panel with a stick or dpad. The panel is a mouse UI. Reading a
   pad to move a synthetic cursor would need `SetCursorPos`, which flips the
   game's input mode and which GGG calls out by name.
2. Rumble. `XInputSetState` exists. We will not use it.
3. Supporting a DualSense over raw HID. If a player wants a DualSense they
   already run Steam Input or DS4Windows, and both present an XInput pad.
4. Any kernel driver. HidHide is not ours to ship or require.
5. Replacing the keyboard hotkey. It stays and stays default.

## Layering, if we build it

| Piece | Layer | Where |
|---|---|---|
| `XInputGetState` calls, slot bookkeeping | adapter | `src/adapter/gamepad_adapter.rs`, `cfg(windows)`, trait `Gamepad` with `automock` |
| Button mask to fired binding | controller, pure | `poe-wayfinder-core/src/controller/gamepad_match.rs` |
| Calling the adapter each frame, opening the panel | driver | the existing `overlay_loop`. No new driver. |
| `ChordBinding`, `GamepadState` | types | `core/src/types/` |

Core stays pure. XInput never appears in `poe-wayfinder-core`.

### Does `hotkey_match` generalise

The shape does. The code does not. Write a sibling, do not extend it.

| `hotkey_match` | Gamepad equivalent |
|---|---|
| `Binding { code, modifiers }` | `ChordBinding { mask: u16 }`. `XINPUT_GAMEPAD.wButtons` is already a u16 bitmask, so a chord is one integer |
| `Reaction::Fire { binding }` | Same, keep it |
| `Reaction::Swallow` | **Delete it. It cannot exist.** |
| `is_modifier_code` | No analogue. A gamepad has no modifier keys |
| `react(KeyEvent)`, edge driven | `react(prev_mask, now_mask)`, level driven. XInput reports held state, not events, so the adapter has no edges and the controller must derive them |

Forcing one type to serve both would put a `Swallow` variant into a world where
nothing can be swallowed. That is a lie in a type, and this workspace has paid
for those before.

## Risks

| Risk | Severity | What it costs |
|---|---|---|
| We cannot swallow a button, so every binding double fires into the game | High | A binding on A opens the panel and swings the character. Only a dead combination is safe, and PoE2 uses almost every button |
| The game may consume the chord itself before we see the effect | High | Unknowable without testing on a live client |
| DualSense reaches XInput only through third party software | Medium | Half the intended audience is served by Steam Input anyway, which makes our adapter redundant for them |
| The mouse anchored panel is wrong for a controller | Medium | Known. `begin_locked` already solves it |
| The advanced descriptions key is not held during copy | Medium | Live bug today. Controller tooltips lose their roll ranges |
| The input mode question in item 3 is unresolved | Medium | Decides whether the panel may use the pointer at all |
| Steam Input takes exclusive control of the pad | Medium | When Steam Input is on, it blocks XInput and DirectInput for everyone else. Our adapter would read nothing. The two approaches do not compose |
| A new gamepad binding conflicts with the game every patch | Low | Ongoing support cost forever |

That last but one risk is decisive. Steam Input and an XInput adapter are
mutually exclusive on the same pad. We would build a feature that stops working
the moment the player uses the better alternative.

## Recommendation

**Phase 0, now, zero code.** Write the Steam Input setup as customer facing
documentation. See `experience.md`. Verify on the machine that a Steam Input
chord mapped to Ctrl+D fires the existing hook. If it does, the user's whole
request is satisfied today.

**Phase 0.5, small and worth doing regardless.** Fix the advanced item
descriptions copy. `keys_to_hold_for_copy` is already ported and already tested
and nothing calls it. This helps every player, not only controller players. It
is a separate scope document and a separate change.

**Phase 1, only if Phase 0 fails.** Build the XInput adapter as laid out above,
default off, with a warning in the settings panel that the game will also see
the button.

## The decision needed

Do we ship Steam Input documentation and stop, or do we build the XInput
adapter knowing it can never suppress the button and stops working the moment
the player enables Steam Input?

## Sources

- https://github.com/Kvan7/Exiled-Exchange-2/discussions/478
- https://kvan7.github.io/Exiled-Exchange-2/nothing-happens.html
- https://github.com/SnosMe/awakened-poe-trade/issues/849
- https://www.pathofexile.com/forum/view-thread/3812578
- https://www.pathofexile.com/forum/view-thread/2077975
- https://www.pathofexile.com/forum/view-thread/3901619
- https://learn.microsoft.com/en-us/windows/win32/xinput/getting-started-with-xinput
- https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw
- https://learn.microsoft.com/en-us/uwp/api/windows.gaming.input
- https://docs.rs/gilrs/latest/gilrs/
- https://github.com/nefarius/HidHide
- https://ds4-windows.com/
- https://steamcommunity.com/sharedfiles/filedetails/?id=3462829061
- https://gamerant.com/path-of-exile-2-use-in-game-price-checker-poe2/
- https://pathofexile2.wiki.fextralife.com/Controls
