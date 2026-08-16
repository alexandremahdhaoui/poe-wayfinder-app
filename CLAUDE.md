
## Architecture

Read `docs/architecture-audit/scope.md`. This crate drifted and was refactored.
The `architecture` forge stage enforces the rules now. When it fails, move the
code. Never raise the floor.

main only builds adapters, injects them into controllers, injects those into
drivers, and starts the drivers. Under 150 lines.

Copy `~/workspaces/playground/golden-rust` for shape. Traits live with their
implementation. `#[cfg_attr(test, mockall::automock)]` sits above the trait.
There is no mocks directory.

## Widget progress is not measured by function parity

`poe-wayfinder-parity` now reads `.vue` script blocks as well as `.ts`, so the
widget directories can be scanned. It still cannot measure them usefully:
`web/overlay` is 1572 lines and yields **two** top level functions, because Vue
composition puts the work inside `setup` as inline arrow handlers rather than
named declarations.

So for the 13 widget backlog the number to watch is not `parity`. It is:

- `poe-wayfinder-uiparity`, which counts user facing capabilities and checks
  each one is reachable from `src/driver/`
- `hack/report.sh`, line "in a widget we do not have", which was 7735 when the
  backlog started

Add the capability to `uiparity` first, watch it fail, then port. That is the
same discipline as widening the parity scan before porting a `.ts` module.

## egui and eframe traps, all paid for

**`ctx.data_mut()` holds the context lock.** Calling `ctx.load_texture()` inside
that closure deadlocks on the first frame. The window opens and never paints,
which looks like a hang. Load first, insert after.

**`run_simple_native` cannot set `clear_color`.** A transparent viewport still
paints an opaque rectangle. An `impl eframe::App` can override it, and even then
a child viewport is not transparent: the splash is made background free by Win32
`WS_EX_LAYERED` + `LWA_COLORKEY` on black. Nothing drawn in it may be `#000000`.

**A blocking call inside the frame callback freezes the whole overlay.** The
price check used to copy, parse and search in sequence there, so nothing
repainted until the network returned. Split it: paint what needs no network,
finish on a later frame.

## The measurement stages are exact about symbols

**`uiparity` `ui:` must name the call site, not the definition.** `fn totals`
never matches `widgets.totals()`. Four capabilities passed review while nothing
called them.

**`parity` `ALIASES` targets must be a bare function name.** The checker looks
for `fn {name}(`, so `"fn wiki"` becomes `fn fn wiki(` and never matches.

**Add the capability before the code and watch the stage go red.** That is the
only thing that proves it was wired rather than renamed.

## The architecture stage will catch these every time

**Never add a wrapper per feature.** `validate`, `validate_with_locked`,
`validate_every`, `validate_all`, `validate_everything`, `validate_with_links` —
each new one stranded the last with no caller, and the stage failed on every
single one. Take a struct of arguments instead.

**A driver may not reach an adapter.** Anything touching `press_combination`,
`LogEvent` or a window handle belongs in `overlay_loop::wiring`, which is waived
because it is the composition root.

**No comments in code.** The reason goes in a test name.

## Harness traps

**Assert on state, not on a transition.** `both-games-check` waited for the game
to change; with both stand-ins open the overlay may already be on the game asked
for and no transition ever appears.

**Never log a provisional value.** `main` reported a game before detection ran,
so the startup line said `poe2` while the driver watched `poe1`.

**Injected input is occasionally swallowed before any window sees it.** A press
check that fails once and passes on retry is that, not a regression. The
harnesses already retry three times.

## The bugs that a user found and the tests did not

**A focused panel IS the foreground window.** `should_draw` required the GAME
to be foreground, so the panel stopped drawing the instant it took focus, then
regained it and oscillated. That one line produced four separate complaints:
the overlay felt clunky, buttons could not be clicked, the locked panel showed
nothing until alt tab, and presses seemed to need repeating. Foreground now
means the game **or any window of our own process**
(`game_window_adapter::foreground_is_ours`).

**Never use `f64::NAN` as an "unset" marker in a widget.** It caused two
user-visible bugs from one line:

- `NaN != NaN`, so egui reported `.changed()` on an empty min/max box **every
  frame**. Both setters force `enabled = true`, so a filter row could never be
  switched off. The user could see the click do nothing; the log showed the
  write succeeding.
- `f64::INFINITY as i64` saturates, so `value.round() as i64` printed
  **9223372036854775807** in the panel.

`format_value` now refuses any non-finite value, and an edit is only emitted
when it actually differs from the row.

**egui `.changed()` is not "the user changed it".** Guard every emit with a
comparison against the value you already hold.

## Instrumentation is the difference between a log and a session

Every bug above needed the user at the keyboard because nothing was logged.
There were 5 `.debug()` call sites in this crate, 0 in core, and 189 of 192
debug lines in a real session were the frame heartbeat.

Core is pure and gets no logger, ever. Log the **data core returns**, at the
driver boundary. `log_request`, `log_estimate`, `log_filter_rows` in
`overlay_loop::win` are the pattern: one line per decision, carrying the inputs
and the chosen output.

The acceptance test for a debug line: **could the last four bugs be diagnosed
from the log alone?**

There is **no `trace` level**. `Level::parse` maps anything unknown to info and
says nothing, so `--log-level trace` silently produces an info log.

## The target is Windows, so the tests run on Windows

`forge test-all` has a `unit` stage and a `unit-windows` stage. The second runs
`cargo test --target x86_64-pc-windows-gnu`, and WSL interop executes the
Windows test binary from this shell. Both crates have it.

Without it, every `cfg(windows)` adapter is compiled by the cross build and
then never run, while the Linux stage happily tests the `cfg(not(windows))`
stub beside it and reports green. That is two suites agreeing about code
neither of them executed.

It found a real one immediately. `SonyPads::buttons()` was asserted to return
`None`, which is true on Linux where the stub returns nothing, true on this
Windows machine today, and **false tomorrow the moment a DualSense is plugged
in**. A test that passes because the hardware is absent fails the day the
hardware arrives. It now asserts the relationship instead: a pad reads as
connected exactly when one is plugged in.

**A test that touches real hardware must hold both with the device and
without.** Assert the relationship, never the absence.

## Run it the way a person runs it, or the output goes nowhere

`hack/windows-smoke.sh`, the `windows-smoke` stage, copies the exe to a real
directory on C: and runs it through cmd and through PowerShell.

It exists because `windows_subsystem = "windows"` means the process starts with
no standard output at all. `attach_console` called `AttachConsole` and stopped
there, so `GetStdHandle` still returned nothing and every `println!` went
nowhere. **Every diagnostic printed nothing when launched from a Windows
shell**, and printed perfectly from WSL, which is the only place anyone had ever
run it. `--list-gamepads`, `--watch-pad` and `--pad-walkthrough` were all
invisible to the one person who was going to use them.

The fix is two more steps after attaching: open `CONOUT$` and `SetStdHandle`.
And skip attaching entirely when a handle is already set, because
`AttachConsole` otherwise overwrites a `> file` redirect with the console and
the file stays empty.

**A gate is only proven by watching it fail.** The old `attach_console` was put
back, rebuilt and run: three FAILs. Then the fix, rebuilt: green. A Windows
side check that has only ever been run against working code is a check nobody
has tested.

## Reading a pad without a crate

`gamepad_adapter` holds two sources. XInput reads Xbox pads. `hid` reads a
DualSense or a DualShock 4 straight from `hid.dll` and `setupapi.dll`, which is
all `hidapi` does on Windows. No crate, four feature strings on `windows`.

**The parse belongs in core, not in the adapter.** The `architecture` stage
refuses one adapter that imports another, and splitting HID transport from Sony
decoding into two adapter files broke that rule seven times over. The decoding
is pure, so it went to `core::controller::sony_pad`, where a captured report
replays through it on Linux with no pad. The stage was right and the code is
better for it.

**A `HANDLE` is neither `Send` nor `Sync`.** `Gamepad` demanded both and the
Linux build never noticed, because the HID code is `cfg(windows)`. Only the
cross build caught it. Nothing needed those bounds, so they went.

**A capture must hold the report from while the button was down.** The first
`--pad-walkthrough` returned the report from the moment of *release*, which
decodes to zero. Every captured line would have been labelled with a button and
held the bytes for nothing at all, and the replay test would have failed against
a capture that took an hour with real hardware to make. Found by reading, not by
a test, because no test can see it without a pad.

**An oracle has assumptions too.** The descriptor path assumes Sony's button
numbering, which is as unverified as the offsets it checks. The button a human
pressed is the ground truth. `hardware-session.md` carries the table that says
which of the two to believe when they disagree.

**The pad is its own oracle.** `HidReader::decode_by_descriptor` decodes a
report through the HID report descriptor the device publishes, using Windows'
own parser, with no offsets from us. `--pad-walkthrough` prints both and
compares. That is how a hardcoded offset table gets checked against reality
without trusting the person who wrote it, and it is how a DualShock 4 owner can
verify offsets nobody here can test.

**A capture is the test.** `--pad-walkthrough` names each button, waits for one
press, and writes the raw reports. The file goes in
`poe-wayfinder-core/tests/fixtures/` and `pad_capture_replay.rs` replays it
forever after. It passes with no fixtures, which is the same trap the empty gem
table was: green while measuring nothing. Until a `.hex` file lands there, the
parser has only ever been tested against reports this repo made up.
