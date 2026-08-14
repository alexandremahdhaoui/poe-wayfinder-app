# Loot filters in the overlay: customer experience requirements

## The headline

A player mid map must be able to change strictness, read why an item was
hidden, and reload the filter, without leaving the game. The target is five
seconds from keypress to loot behaving differently. Today it takes 30 to 60
seconds and a trip through a browser.

Every flow below runs on files on the player's own disk. None of them calls a
third party.

Read `scope.md` in this directory first. It carries the research and the source
URLs. This document is the customer requirements only.

## 1. The problem today

### 1.1 The game does not watch the file

The game reads a `.filter` file once, when it loads it. An edit on disk changes
nothing until a reload. The player must open Options, Game, Item Filter and
press the refresh button.

- Source: https://skycoach.gg/blog/path-of-exile-2/articles/item-filter-guide

That is the root of the clunkiness. Every other complaint below is a
consequence of it.

### 1.2 Reloading is unreliable and players trade folk remedies

Players pass around workarounds because the reload behaviour is not obvious.
Two circulate widely.

- Do not alt tab until the filter finishes reloading after entering a map. The
  reload takes 1 to 2 seconds.
- After each game start, open Options and click the filter list once. Skip that
  and the filter does not reload on entering a new area.

- Source: https://steamcommunity.com/app/238960/discussions/0/4353366523983184056

A player following a ritual to make a text file load is a player with a broken
tool.

### 1.3 Switching strictness means going back to the web

The standard advice is to return to FilterBlade and generate a new file when
you progress into maps.

- Source: https://maxroll.gg/poe/getting-started/lootfilter

That is a browser trip, a download, a file move, and a menu trip, per change.
Strictness is a thing players want to nudge several times in a session. The
cost per nudge is why they stop nudging and live with a filter that is wrong.

### 1.4 The filter is opaque about its own decisions

Nothing tells a player why an item did not appear. The answer sits in one of
several thousand lines of a `.filter` file. Players respond by editing the
whole filter rather than the one rule.

Community reports describe exactly this. Players take NeverSink's filter, make
it stricter by hand, decouple sections and change highlights, because the
supplied strictness levels do not match what they want to see.

- Source: https://steamcommunity.com/app/2694490/discussions/0/598517735210639465

Hand editing then makes the next FilterBlade regeneration destructive, which is
the trap section 6 protects against.

### 1.5 Tool builders already solved a piece of this, badly

`PoEDynamicLootFilter` exists because the need is real. It modifies the filter
while you play and reloads it from a hotkey. It needs AutoHotkey and Python 3
installed, and it warns that the less you have moved things around or written
custom rules, the better it works.

- Source: https://github.com/Apollys/PoEDynamicLootFilter

The demand is proven. The delivery is a two runtime install that is fragile
against hand edits. We ship one exe and we do not break hand edits.

### 1.6 What the game already gives us that nobody surfaces

`/itemfilter <name>` switches to a named filter instantly, in both games.
`/reloaditemfilter` reloads the active filter and arrived in PoE1 patch 3.23.1.

- Source: https://game8.co/games/Path-of-Exile-2/archives/496264
- Source: https://github.com/ChaosRecipeEnhancer/ChaosRecipeEnhancer/issues/581

Neither command is discoverable. Players do not know they exist. The overlay
already sends chat commands. Putting a button in front of `/itemfilter` removes
the menu trip entirely.

## 2. Jobs to be done

Each job is one thing a player wants at one moment, mid map, with the overlay
open.

| # | Job | Trigger |
|---|---|---|
| J1 | Know which filter the game has loaded right now | Screen looks wrong. Player doubts what is active. |
| J2 | Go one step stricter because the screen is full of junk | Player entered maps. Ground clutter costs time. |
| J3 | Go one step softer because something valuable got hidden | Player suspects the filter ate a drop. |
| J4 | Understand why the item I just copied was hidden or highlighted | Player sees an odd colour, or sees nothing at all. |
| J5 | Show one thing the filter is hiding, for this session | Player is farming one base type. |
| J6 | Reload after editing the file in a text editor | Player edited by hand and wants it live. |
| J7 | Know my filter is stale | New league started. Filter is from the last one. |

J1 through J6 need no network. J7 needs only the file's modified date to warn,
and a network host to fix.

## 3. The interactions

The panel is a new `Filter` tab in the existing widget system. It opens the way
every other tab opens. The overlay already follows the foreground window
between PoE1 and PoE2, so the tab always shows the game the player is actually
in.

### 3.1 Flow A. See what is active

| Step | User does | User sees | Overlay does |
|---|---|---|---|
| 1 | Presses the overlay hotkey and picks the Filter tab | The tab opens with a header line | Reads the game's filter directory. Lists every `.filter` file. |
| 2 | Nothing | `Active: 3-STRICT` and `PoE2` and `edited 6 days ago` | Shows the filter it last switched to. If it has not switched one this session, it shows `Active: unknown` and lists the candidates. |
| 3 | Nothing | A ladder of the files it recognises, ordered SOFT to UBER-PLUS-STRICT, with the current one marked | Orders by the NeverSink naming scheme. Files it cannot place go in an `Other` list in alphabetical order. |

The overlay never claims to know the active filter when it does not. `unknown`
is an honest answer and appears until the player switches once through the
overlay.

### 3.2 Flow B. Change strictness

| Step | User does | User sees | Overlay does |
|---|---|---|---|
| 1 | Clicks `Stricter` or presses the bound key | The next rung highlights | Picks the next file up the ladder. Does not write anything. |
| 2 | Nothing | `Switching to 4-VERY-STRICT` | Sends `/itemfilter 4-VERY-STRICT` through the existing chat driver. One keypress, one action. |
| 3 | Nothing | The header updates to `Active: 4-VERY-STRICT` and the chat line the game printed | Records the new active name. Loot on the ground changes on the next drop. |

No file is written. No file is read beyond the directory listing. Strictness
switching is pure navigation over files the player already has.

If only one filter file exists, `Stricter` and `Softer` are disabled and the
panel says `Only one filter found. Add more files to <path> to switch between
them.`

### 3.3 Flow C. Explain an item

| Step | User does | User sees | Overlay does |
|---|---|---|---|
| 1 | Hovers an item and presses the price check hotkey | The normal price panel, plus a new `Filter` line | Parses the clipboard with the existing item parser. That output is already the right shape. |
| 2 | Clicks the `Filter` line | The explain view | Matches the parsed item against the parsed blocks of the active filter. |
| 3 | Nothing | `Hidden by block at line 2841` and the block's own text, with the conditions that matched shown in bold | Reports every block that touched the item in order, because `Continue` means more than one can. |
| 4 | Nothing | `Show. Line 1204. SetTextColor 255 190 0. MinimapIcon 1 Yellow Circle` for a styled item | Names the actions the block applied. |

If no block matched, the panel says `No block matched. The game shows this item
with default styling.` That is a real filter outcome and not an error.

### 3.4 Flow D. Toggle one rule

This is the only flow that writes a file. It asks first, every time.

| Step | User does | User sees | Overlay does |
|---|---|---|---|
| 1 | In the explain view, clicks `Show this instead` on a Hide block | A confirm row appears | Writes nothing yet. |
| 2 | Nothing | `This edits <filename> at line 2841. A backup goes to <filename>.wayfinder-backup. Continue?` with `Write` and `Cancel` | Waits. |
| 3 | Clicks `Write` | `Wrote 1 change. Reloading.` | Copies the file to the backup name. Writes the file back with exactly one word changed and every other byte identical. |
| 4 | Nothing | The header confirms the reload | Sends `/itemfilter <active name>`. The game reloads. |
| 5 | Later, clicks `Undo last change` | `Restored <filename> from backup.` | Restores from the backup and reloads again. |

The confirm row names the file and the line before anything happens. There is
no silent write anywhere in this design.

### 3.5 Flow E. Reload after a hand edit

| Step | User does | User sees | Overlay does |
|---|---|---|---|
| 1 | Edits the filter in a text editor, alt tabs back | The panel shows `File changed on disk 4 seconds ago` | Compares the file's modified time to the last one it read. It does not reload on its own. |
| 2 | Clicks `Reload` | `Reloading 3-STRICT` | Sends `/itemfilter 3-STRICT`. |
| 3 | Nothing | `Reloaded` and a re parsed explain view | Re parses the file in the background so the next explain is correct. |

Step 1 is a notice, not an action. The overlay never reloads because a file
changed. Section 7 explains why that line matters.

### 3.6 Flow F. Stale filter warning

| Step | User does | User sees | Overlay does |
|---|---|---|---|
| 1 | Opens the Filter tab | `3-STRICT is 94 days old. Filters usually update every league.` | Reads the file's modified date. Nothing more. |
| 2 | Nothing | No download button in the default build | Nothing. The default build adds no host. |

If the user approves a host later, this is where a `Fetch latest` button goes.
It is the only place in the design that would need one.

## 4. Acceptance criteria

Each is measurable and each fails a build if it regresses.

| # | Criterion | How it is measured |
|---|---|---|
| A1 | A strictness change is live in the game within 5 seconds of the keypress, with no alt tab | Time from press to the game's own filter reload chat line. The command itself is instant. The game's reload takes 1 to 2 seconds. |
| A2 | The Filter tab opens in under 100 ms with a 10000 line filter already parsed | Frame time on the tab switch. Parsing happens off the frame loop and is cached against the file's modified time. |
| A3 | A first parse of a 10000 line filter finishes in under 500 ms and never blocks a frame | Timed on a background thread. Same rule the data refresh already follows. |
| A4 | An explain answer appears in under 50 ms once the filter is parsed | Matching is pure and runs against an in memory block list. |
| A5 | Writing a filter and reading it back yields byte identical output when no edit was requested | Property test over a corpus of real filters. Parse, write, compare bytes. |
| A6 | Zero writes happen without a confirm click | Every write path goes through one function. A test asserts no other caller writes. |
| A7 | Zero reloads happen without a keypress or a click | Every send goes through one function. A test asserts no timer, no file watcher and no zone change triggers it. |
| A8 | The panel works identically in PoE1 and PoE2 with no user action on game switch | The both games harness. Open a stand in for each game and assert the panel follows the foreground window. |
| A9 | An unknown filter condition never fails a parse | Corpus test with injected garbage lines. The line survives round trip and is reported as unrecognised. |
| A10 | The overlay makes no outbound request for any flow in section 3 | The allowlist stays `www.pathofexile.com`. A test asserts the filter controller has no http adapter. |

A1 is the headline number. 60 seconds becomes 5.

## 5. Failure modes

Every message names the thing that failed and what the player can do. No error
codes. No silence.

| Failure | What the user sees | What the overlay does |
|---|---|---|
| Filter directory not found | `No filter folder found. Looked in <full path>. Start the game once to create it.` | Shows the exact path it tried. Offers nothing else. Does not create the directory. |
| Directory found, no `.filter` files | `No filters in <full path>. Put a .filter file there and press Refresh.` | Lists the directory so the player can see what is actually in it. |
| Malformed filter, unknown condition | `Line 2841 uses a condition I do not recognise. I left it alone.` The block still appears in explain, marked `partly understood`. | Keeps the line verbatim. Never drops it. Never writes it back changed. Explain output for that block is marked incomplete rather than wrong. |
| Malformed filter, broken structure | `<filename> does not parse. Line 91 opens a block that never closes. Nothing was changed.` | Refuses to explain and refuses to write. Read only from that point. Switching to a different filter still works. |
| `Import` target missing and not `Optional` | `<filename> imports <name>.filter which is not there. The game will refuse this filter too.` | Reports it the way the game would. Does not guess. |
| `Import` cycle | `<filename> imports itself through <chain>. I stopped.` | Names the chain. Stops rather than looping. |
| Game will not reload | `Sent /itemfilter 3-STRICT. The game did not confirm. Try Options, Game, Item Filter, refresh.` after 5 seconds | Falls back to telling the player the manual route. Does not resend. Does not retry on a timer. |
| Chat is not reachable, game not focused | `The game is not in focus. Click the game and press again.` | Does not send. A chat command sent to the wrong window types into whatever is there. |
| Filter name has a space | `/itemfilter` receives the name as the game expects it. If the switch fails, the panel says `The game did not accept that name. Rename the file without spaces.` | Sends the file name without the extension, unchanged. |
| Local and online filter share a name | Both listed. The local one is marked `local`, the online one `online`. `Two filters are called 3-STRICT. /itemfilter may pick the wrong one.` | Warns before switching. This is the exact conflict the ChaosRecipeEnhancer issue describes. |
| File is read only or locked | `Cannot write <filename>. It is read only.` | Reports and stops. Does not change permissions. |
| Backup cannot be written | `Cannot write the backup next to <filename>. I did not change anything.` | Refuses the edit. No backup means no write. |
| PoE1 versus PoE2 condition mismatch | `LinkedSockets is a PoE1 condition. This is a PoE2 filter.` | Explains per game. The condition table splits by game the way the item parser already splits into shared, poe1 and poe2. |
| `/reloaditemfilter` missing in PoE2 | Nothing. The player never sees this. | Always uses `/itemfilter <active name>`, which works in both games. `/reloaditemfilter` is never sent. |
| Active filter unknown | `Active: unknown` and every candidate listed | Never guesses. Nothing tells us what the game loaded. Reload and explain are disabled until the player picks one. |

## 6. Safety

Three rules. None of them is negotiable.

**Never write a file the user did not ask for.** Every write is preceded by a
confirm row that names the exact file path and the exact line. One click, one
write. No batching. No "apply all". A single function owns every write and a
test asserts nobody else calls the file system to write.

**Never silently overwrite a hand edit.** Three protections stack.

1. The overlay copies the file to `<name>.wayfinder-backup` before every write.
   No backup means no write.
2. The write is a surgical splice. Exactly the requested span changes. Every
   other byte is identical, including whitespace, comment lines the player
   wrote, and conditions we did not understand. The property test in A5 proves
   it.
3. The overlay re reads the file's modified time immediately before writing. If
   it changed since the last parse, the write is refused with `<filename>
   changed on disk since I read it. Press Refresh and try again.` That closes
   the window where a text editor save and an overlay write race.

**Never touch what is not ours.** The `onlinefilters` subdirectory is read
only, always. The overlay reads it to name filters and never writes into it.
The game ignores hand placed files there, so writing there would produce a file
that silently does nothing.

Accessibility notes:

- Every action in the panel has a keyboard route. Nothing needs the mouse.
  `Stricter` and `Softer` get bindable keys.
- Colour is never the only signal. The explain view marks Show and Hide with
  the words `Show` and `Hide`, not with green and red alone.
- The panel reports numbers as text the player can read and repeat in a support
  question. Paths are shown in full, never truncated.

## 7. GGG's third party rules

Everything in section 3 fits GGG's stated rules. One line decides it and we
stay well behind it.

What GGG says:

- Programs that interact with the game client are not allowed. Tools that are
  entirely external, such as ones that read the client log files, are fine.
- Input automation must follow the macro rules. A macro must be invoked
  manually by the user. Automatic invocation from timers, from reacting to file
  changes, or from reading the screen is not allowed. One invocation performs
  one action. Sending a single chat message counts as one action.
- GGG will not certify any tool. They state they cannot comment on the legality
  of third party tools and cannot guarantee one stays allowed.

- Source: https://www.pathofexile.com/legal/terms-of-use-and-privacy-policy
- Source: https://www.pathofexile.com/forum/view-thread/3584808

Applied to each flow:

| Flow | Verdict |
|---|---|
| A. See what is active | Reads a directory. Entirely external. Fine. |
| B. Change strictness | One chat message from one keypress. This is exactly what the macro rules permit. |
| C. Explain an item | Reads the clipboard the player filled with the game's own copy. The existing price check already does this. Fine. |
| D. Toggle one rule | Writes a file the player named, then one chat message from one click. Fine. |
| E. Reload after a hand edit | One chat message from one click. Fine. The file change notice is a display, not a trigger. |
| F. Stale warning | Reads a file date. Fine. |

**On the specific question of modifying a filter while the client runs.** We
found nothing in GGG's rules that treats this differently from editing the file
with the game closed. The game reads the file on reload and does not hold a
lock on it. `PoEDynamicLootFilter` has done this for years in public.

- Source: https://github.com/Apollys/PoEDynamicLootFilter

**The line we must not cross is the invocation rule.** Reloading because a file
changed is explicitly named as not allowed. So is reloading on a timer. So is
reloading because the screen changed. Flow E shows a notice and waits for a
click for that exact reason. Criterion A7 is the test that keeps it true.

**GGG certifies nothing.** No design here can be called approved. The honest
statement is that every flow sits inside the rules GGG published, and that GGG
reserves the right to change them.

## 8. The FilterBlade decision, in one place

`scope.md` section 11.1 carries the detail. Here is the question in one line.

**Do you want the overlay to call filterblade.xyz, and if so, do we ask the
maintainer first?**

Pick one.

| Answer | What we do next |
|---|---|
| **Ask first** | I draft one message to NeverSink asking for a documented endpoint and permission for a free desktop overlay. We ship sections 1 through 7 while waiting. |
| **Approve the host** | You name `filterblade.xyz` for the allowlist. We build against public pages only, with no documented contract and no permission. |
| **Stay local** | We ship sections 1 through 7 and stop. Nothing calls out. |

**Recommendation: ask first.** Every job in section 2 already ships without
them. FilterBlade publishes no API, forbids modification of its code without
permission, reserves the right to block access, and charges 3 US dollars a
month for automated delivery because it loads their servers. One message turns
all of that from a risk into an agreement. A no costs us nothing, because the
feature already shipped.

`api.exiledexchange2.dev` is permanently banned and is not an option in any
answer.
