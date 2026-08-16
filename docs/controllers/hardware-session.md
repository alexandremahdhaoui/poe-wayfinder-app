# The hardware session

Everything a pad can prove, in the order that makes each step worth running.
A step that fails stops the run. The step after it would measure nothing.

Written for 2026-08-17, a DualSense and an Xbox pad.

## Build and deploy

```sh
cd poe-wayfinder-app
touch src/bin/poe-wayfinder.rs
cargo build --release --target x86_64-pc-windows-gnu -p poe-wayfinder-app --bin poe-wayfinder
bash hack/deploy.sh
```

Kill any running overlay first or the copy fails with `Permission denied`.
Smart App Control blocks roughly two new unsigned builds in three. If the exe
will not start, touch a source file, rebuild so the hash moves, deploy again.
Retrying the same binary is useless.

Run the commands below from PowerShell, in the deploy directory. They print
there now. Until 2026-08-16 they printed nothing at all from a Windows shell
and only worked from WSL, which is what `hack/windows-smoke.sh` now guards.

## 1. The exe is allowed to run

```
./poe-wayfinder-<hash>.exe --list-windows
```

Anything printed means SAC let it through. Nothing else is testable until this
passes.

## 2. The DualSense is visible, over USB

Plug it in by cable.

```
./poe-wayfinder-<hash>.exe --list-gamepads
```

Expected: one line reading `0x054c:0x0ce6  64 bytes  DualSense`.

| What is printed | What it means |
|---|---|
| the line above | enumeration works. Go on |
| nothing at all | Windows is not exposing it as a gamepad collection, or another program has it |
| the pad, but 0 bytes | `HidP_GetCaps` failed. The report length is wrong and reads will be empty |
| a different report length | write it down. The parser picks its offsets from that length |

## 3. The buttons decode

```
./poe-wayfinder-<hash>.exe --watch-pad 30
```

Press Square, Cross, Circle, Triangle, L1, R1. Each line prints the raw report
and the name this build decodes. The names must match the labels on the pad.

If Cross prints as `SQUARE`, an offset is off by one. Do not continue.

## 4. The walkthrough, which is the real measurement

```
./poe-wayfinder-<hash>.exe --pad-walkthrough dualsense-usb.hex
```

It names sixteen buttons. Press each one alone and let go. At the end it prints
a table with four columns and a verdict per row.

The **descriptor** column decodes the same press a second time, through the
pad's own HID report descriptor, using the parser Windows itself provides. That
number comes from what the pad says about itself rather than from any offset we
wrote down.

**The button you physically pressed is the ground truth, not either column.**
The descriptor path has an assumption of its own: that Sony numbers buttons 1
to 12 as Square, Cross, Circle, Triangle, L1, R1, L2, R2, Create, Options, L3,
R3. That is `bit_for_button` in core and it is unverified too.

| expected vs read | read vs descriptor | What is wrong |
|---|---|---|
| same | same | nothing. The offsets are confirmed by the hardware |
| differ | same | both paths are wrong the same way, which is close to impossible. Suspect the label you pressed |
| differ | differ | our offset table. Fix `parse_report` |
| same | differ | `bit_for_button`, the Sony numbering. Our offsets are fine |

The row reads FAIL on any disagreement, so read the table before changing
code.

Every row must read PASS and the disagreement count must be zero. Copy
`dualsense-usb.hex` into `poe-wayfinder-core/tests/fixtures/`.

That file is the point of the whole session. Once it is committed the parser is
regression tested forever with no pad and no Windows.

## 4b. The second opinion, through WSL

Approved on 2026-08-16: usbipd only, and we wrote the recorder ourselves. No
third party code reads the pad.

One time setup. In an administrator PowerShell:

```
winget install usbipd
usbipd list
usbipd bind --busid <the DualSense busid>
```

In WSL:

```sh
sudo apt install linux-tools-generic hwdata
```

Then, each session:

```
usbipd attach --wsl
```

While attached the pad belongs to WSL and Windows cannot see it, so do this
**after** step 4, never before. `usbipd detach` gives it back.

In WSL:

```sh
ls /dev/hidraw*
cd poe-wayfinder-app
cargo run --bin poe-wayfinder -- --record-hidraw /dev/hidrawN dualsense-wsl.hex
```

It asks for the same sixteen buttons and writes the same format. hidraw usually
needs sudo or a udev rule.

Now compare. The two files were produced by two different code paths on two
different operating systems reading the same hardware:

```sh
diff <(grep -v '^#' dualsense-usb.hex) <(grep -v '^#' dualsense-wsl.hex)
```

The bytes will differ in the counter and timestamp fields. What must match is
the verdict table: the same button must decode to the same bit in both. If it
does not, one path is wrong and today is the cheapest day to find out.

The descriptor column reads `not read` on Linux. That oracle is Windows only.
This step is a different oracle, not the same one twice.

## 5. The same pad over Bluetooth

Unplug the cable. Pair it. Repeat steps 2 to 4 into `dualsense-bt.hex`.

**Write down which report id arrives.** Research could not settle whether
Windows hands out the 10 byte report `0x01` or the 78 byte report `0x31`. The
first line of `--watch-pad` answers it. Both are handled, so either is fine.
The unknown is which, not whether.

## 6. The Xbox pad

Plug it in.

`--list-gamepads` must **not** show it. An Xbox pad is read through XInput, not
HID. Its absence here is the correct answer, not a failure.

## 7. Both pads at once

Leave both connected. Start the overlay:

```
./poe-wayfinder-<hash>.exe --gamepad-chord "L1+R1+Triangle" --log-level debug
```

Open the status window from the tray. The Controller row must read
`L1+R1+TRIANGLE, pad connected`.

Hold the chord on the DualSense. Then hold LB+RB+Y on the Xbox pad. Both must
fire. One chord, either pad, written once in whichever names you prefer.

## 8. A whole price check from the pad

```sh
bash hack/press-check.sh <exe> "" poe2 item.txt
```

That proves the keyboard path still works. Then by hand, with `--fake-game`
holding a stand in window, hold the chord and watch for
`price check finished` with a non zero `stat_rows`.

## 9. The suite

```sh
bash hack/check-all.sh <exe>
```

Nine harnesses, none of which can inject a gamepad press, because Windows
offers no way to synthesise one. What this proves is that the pad code did not
break the keyboard path. Run it with the game closed.

## 10. The last open question, which needs the game

Start Path of Exile 2. Find a chord the game does nothing with.

Windows cannot hide a pad button from the game, so every button in the chord
reaches the character as well. L1+R1+Triangle was chosen as a starting guess,
not as an answer. If the game acts on it, try the touchpad click or a
combination the game leaves alone.

This is the only question in this document that code cannot settle.

## After

- Commit the `.hex` captures. Run `forge test-all` in core and watch
  `pad_capture_replay` go from vacuous to real.
- Set `T5`, `T6`, `T7`, `T10`, `T11` and `T13` to VERIFIED in `plan.yaml`, but
  only the ones the session actually exercised.
- Delete the entry from **Waiting on a human** in `FOLLOWUP.md` only after the
  chord fires in the real game.
