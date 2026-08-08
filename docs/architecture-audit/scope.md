# Architecture audit: poe-trader-app

Measured 2026-08-08. Every number is from the tree, not from memory.

## The finding

`poe-trader-core` is genuinely hexagonal. `poe-trader-app` is hexagonal in
folder names only.

The directories are right. The dependencies are not. Nothing enforces the
layering, so it did not happen.

## What was measured

| Thing | Rule | Actual |
|---|---|---|
| `src/bin/poe-trader.rs` | main wires and starts drivers | **2091 lines**, 157 of them tests |
| `run_overlay` | is a driver | ~700 lines, inside the binary |
| `search` | is a controller | inside the binary |
| App adapters with a port trait | all | **4 of 14** |
| Drivers with a port trait | all | **0 of 6** |
| `#[automock]` uses | mocks are generated | **0** |
| Hand-written fakes | none | **12** |

`mockall` is a declared dependency of both crates and is used nowhere.

## What this costs

**main is the application.** `run_overlay` holds the frame loop, the tray
dispatch, the price check orchestration, the placement, the lifecycle and the
logging. It cannot be unit tested at all. Every bug found this week lived
there: the frame loop stopping, the double press, the panel off screen, the
dismissal that closed the process. None of them could have been caught by a
unit test, because there is nothing there to call.

**Drivers take concrete types.** `overlay_ui_driver`, `tray_driver` and the
hotkey drivers are called directly by the binary rather than injected as
interfaces. A driver cannot be tested against a fake controller, so driver
behaviour is only ever tested through the real one, or not at all.

**Ten adapters have no port.** A controller wanting the clock, the browser, the
filesystem or the window state calls a free function. It cannot be given a fake
one, so the controller cannot be tested without the real thing.

**Coverage hid it.** `poe-trader-core` is pure and heavily tested by value, and
it is most of the line count. The app crate's few controllers use hand-written
fakes. So the totals looked healthy while the layering did not exist.

## Why it drifted

Every step was locally reasonable. A diagnostic here, a flag there, one more
branch in the frame loop. Nothing in the build ever said "this belongs in a
driver", because nothing measures it. Parity counts ported functions and
`datacheck` counts stats. Neither looks at the shape of the code.

That is the actual lesson: **the architecture is not measured, so it rots the
same way PoE1 parity rotted before `parity-poe1` existed.**

## Goals

1. main under 150 lines: read config, build adapters, inject, start.
2. Every adapter and every driver reaches its caller through a trait.
3. Mocks generated with `mockall`, hand-written fakes deleted.
4. A forge stage that fails the build when this drifts again.

## Non-goals

- Rewriting `poe-trader-core`. It is already right.
- Changing behaviour. This is a refactor; the press-check harness and
  `forge test-all` must pass unchanged at every step.
- Touching the data pipeline or the parser.
