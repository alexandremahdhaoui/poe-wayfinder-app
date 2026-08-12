
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
