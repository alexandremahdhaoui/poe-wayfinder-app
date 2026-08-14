# Loot filters in the overlay: scope

## The headline

The overlay manages loot filters entirely from local files. It calls nothing.
This is the whole design, not the first of several options.

Two findings make it work, and both are already in the codebase.

**One. `/itemfilter <name>` switches filters in both PoE1 and PoE2, instantly.**
The overlay already sends chat commands through
`poe-wayfinder-core/src/controller/chat.rs` and
`poe-wayfinder-app/src/driver/chat_driver.rs`. Switching strictness becomes one
keypress instead of a browser trip and a menu trip.

**Two. `game_config_adapter.rs` already resolves the filter directory.** It
builds `Documents\My Games\Path of Exile` and
`Documents\My Games\Path of Exile 2` today, for the game config ini. Filters sit
in that same directory. There is no path discovery work to do.

Everything else follows. A `.filter` file is line oriented text, so parsing it,
explaining a match and splicing one edit are all pure functions that belong in
`poe-wayfinder-core`.

The allowlist does not change. It stays `www.pathofexile.com`. The overlay never
downloads a filter. Section 11 records the decisions behind that and why they
are closed.

## 1. Problem

A player mid map cannot change anything about their loot filter without leaving
the game.

Today the loop is: alt tab out, open a browser or a text editor, find the
filter file, edit or re download it, alt tab back, open Options, find the
filter dropdown, click reload, and hope it took. That costs 30 to 60 seconds
and breaks the map.

Three specific things the player cannot do today:

| Want | Today |
|---|---|
| Go one strictness step up because the screen is full of junk | Alt tab, download a different file, select it in Options |
| Know which filter is actually loaded right now | Alt tab to Options and read the dropdown |
| Understand why an item on the ground was hidden | Open the .filter file in a text editor and read 5000 lines |
| Unhide one base type for one map | Hand edit the file, then reload |

The overlay is already on screen, already knows which game is in focus, already
reads that game's config directory, and already sends chat commands. Every
piece needed is present.

## 2. Goals

1. Show the player which filter file the game has loaded, without alt tabbing.
2. Let the player switch to another filter in the same directory with one
   click, and have the game pick it up within a few seconds.
3. Explain any item the player copies: which Show or Hide block matched it, and
   what that block did to it.
4. Let the player toggle a named rule between Show and Hide, write the change,
   and reload the game's filter, without alt tabbing.
5. Never damage a filter the player wrote by hand.
6. Work for PoE1 and PoE2 from one parser, the way the item parser already
   does.
7. Add no outbound host. The allowlist stays `www.pathofexile.com`.

## 3. Non goals

- **The overlay never downloads a filter.** Not from FilterBlade, not from
  GitHub, not from anywhere. Section 6 records that decision and section 8
  covers where filter files come from instead.
- No filter authoring from scratch. We edit and explain filters the player
  already has. We do not become a filter generator.
- No sound or visual preview of alert sounds and beams. We report what a block
  says, we do not render the game's presentation.
- No writing to online filters stored on the GGG account. Local files only.
- No OAuth and no account sync. Section 7.1 records why.
- No automation. The overlay never reloads a filter on a timer, on a zone
  change, or on a file change. Every write and every reload comes from a
  keypress the player made. Section 7 explains why this line is not negotiable.

## 4. Research: what a loot filter is

### 4.1 The file format

GGG publishes the full item filter reference. It is the source of truth.

- Source: https://www.pathofexile.com/item-filter/about

Findings:

| Element | Detail |
|---|---|
| Block types | `Show`, `Hide`, and `Minimal`. `Minimal` is Ruthless only and sets a minimum size transparent label. |
| Matching | A block matches an item only when every condition in the block matches. First matching block wins. |
| `Continue` | A `Show`, `Hide` or `Minimal` block may end with `Continue` so matching carries on past it. This is why "which block matched" is a list, not one answer. |
| `Import` | `Import "name.filter"` pulls another file in. `Import "name.filter" Optional` skips it when missing. A parser that ignores `Import` reads the wrong filter. |
| Conditions | Over 70. Rarity, BaseType, Class, ItemLevel, AreaLevel, Quality, Sockets, LinkedSockets, SocketGroup, Corrupted, Identified, Mirrored, FracturedItem, GemLevel, GemQualityType, map mods, and the six influence types. |
| Actions | `SetTextColor`, `SetBackgroundColor`, `SetBorderColor`, `SetFontSize`, `PlayAlertSound`, `CustomAlertSound`, `MinimapIcon`, `PlayEffect`. |

The grammar is line oriented and indentation is decoration. That makes it a
cheap parse and a cheap round trip.

### 4.2 Where the files live

| Game | Directory |
|---|---|
| PoE1 | `%USERPROFILE%\Documents\My Games\Path of Exile\` |
| PoE2 | `%USERPROFILE%\Documents\My Games\Path of Exile 2\` |

The overlay already resolves both of these. `game_config_adapter.rs` builds
`documents/My Games/<Path of Exile or Path of Exile 2>/<config file>` in
`config_dir_name` and `candidate_paths`. The filter directory is the same
directory as the config ini, one level up from the file name. No new path
discovery work is needed.

- Source for PoE2: https://skycoach.gg/blog/path-of-exile-2/articles/item-filter-guide
- Source in repo: `poe-wayfinder-app/src/adapter/game_config_adapter.rs` lines 31 to 44

Both games also keep an `onlinefilters` subdirectory. That holds filters synced
down from the GGG account. Guides warn against dropping a hand made file in
there because the game ignores it. We read that directory to name what is
loaded. We never write into it.

- Source: https://skycoach.gg/blog/path-of-exile-2/articles/item-filter-guide

### 4.3 Selecting and reloading

The player picks a filter in Options, Game, Item Filter. The dropdown lists
what is in the filter directory plus the account's online filters.

The game does not watch the file. An edit on disk does nothing until a reload.
Three ways to reload exist and two of them are chat commands.

| Method | Game | Note |
|---|---|---|
| Options, Game, Item Filter, refresh button | PoE1 and PoE2 | Needs alt tab or at least a menu trip |
| `/itemfilter <name>` | PoE1 and PoE2 | Switches to the named filter immediately. Documented in the PoE2 command list as "Allows you to specify a filter by name to instantly switch to using it." |
| `/reloaditemfilter` | PoE1, added in patch 3.23.1 | Reloads the filter that is already active. Not present in the PoE2 command list we found. |

- PoE2 command list: https://game8.co/games/Path-of-Exile-2/archives/496264
- `/reloaditemfilter` introduction and behaviour: https://github.com/ChaosRecipeEnhancer/ChaosRecipeEnhancer/issues/581
- `/reloaditemfilter` usage: https://www.vhpg.com/poe-reload-your-item-filter/

This is the finding that makes the whole feature work. `/itemfilter <name>`
exists in both games. It both switches and reloads. Where `/reloaditemfilter`
is missing, `/itemfilter <currently active name>` does the same job. The
overlay already sends chat commands through
`poe-wayfinder-core/src/controller/chat.rs` and
`poe-wayfinder-app/src/driver/chat_driver.rs`.

### 4.4 PoE1 versus PoE2 differences that matter

| Difference | Effect on us |
|---|---|
| Directory name | Already handled by `config_dir_name` |
| `/reloaditemfilter` may be PoE1 only | Use `/itemfilter <name>` as the one path that works in both |
| PoE2 conditions differ. No links, no six link sockets, different item classes | The condition table must be per game, the way the parser stages already split into `shared/`, `poe1/`, `poe2/` |
| PoE2 filters are younger. GGG still adds conditions each patch | Unknown conditions must be preserved untouched, never dropped |

## 5. Research: NeverSink

| Item | Finding | Source |
|---|---|---|
| PoE1 repo | `NeverSinkDev/NeverSink-Filter` | https://github.com/NeverSinkDev/NeverSink-Filter |
| PoE2 repo | `NeverSinkDev/NeverSink-Filter-for-PoE2` | https://github.com/NeverSinkDev/NeverSink-Filter-for-PoE2 |
| Licence | MIT on both | Both repo pages |
| Strictness levels | Seven. `0-SOFT`, `1-REGULAR`, `2-SEMI-STRICT`, `3-STRICT`, `4-VERY-STRICT`, `5-UBER-STRICT`, `6-UBER-PLUS-STRICT` | https://github.com/NeverSinkDev/NeverSink-Filter-for-PoE2 |
| Release cadence | Updated 4 to 6 hours before a new league starts, then every few weeks | Both repo READMEs |
| Programmatic fetch | `.filter` files are published as GitHub Release assets at stable URLs | https://github.com/NeverSinkDev/NeverSink-Filter-for-PoE2 |
| Auto update | The repo README states GitHub filters do NOT auto update and points at the paid FilterBlade auto updater | https://github.com/NeverSinkDev/NeverSink-Filter-for-PoE2 |

The seven level naming is what makes an offline strictness ladder possible. It
gives us an ordered ladder purely from file names in the player's own
directory. No network call is needed to offer "one step stricter" once the
player has more than one of these files on disk. That is the common case,
because every route that puts a NeverSink filter on disk names the files this
way.

## 6. FilterBlade: decided, local only, permanently

**The overlay never calls filterblade.xyz. We never ask its maintainer either.
The user decided this on 2026-08-14 and it is now a hard rule in the workspace
`CLAUDE.md`. This section is the record, not an open question. Do not reopen
it.**

Three facts carried the decision, and the research below is the evidence.

1. **There is no documented public API.** Nothing to call, nothing to depend
   on, and no promise that anything stays put.
2. **Their published terms require explicit permission to redistribute or
   modify their work.** They also reserve the right to block access for any
   reason.
3. **Every job in section 2 ships without them.** Filters are local files. The
   feature loses nothing.

The third fact is the one that closed it. Sections 1 through 5 and 8 through 10
describe a complete feature that calls nobody.

`api.exiledexchange2.dev` is a separate matter and is also permanently banned.
It is not an option for anything.

### 6.1 What FilterBlade is

FilterBlade is the web customiser for NeverSink's filters. It is built and run
by NeverSink, Zoey and Haggis. It does what raw filters do not: a visual editor
over the rule set, per category tiering, style and sound editing with live
preview, and a paid auto updater that pushes new filter versions to a player's
account.

- Source: https://www.filterblade.xyz/
- Source: https://www.filterblade.xyz/Contact

### 6.2 Is there a documented public API

No. We looked at what FilterBlade publishes and found no API documentation, no
developer page, and no published endpoint contract.

We did not probe for one. We did not fetch its JavaScript bundles, did not
enumerate paths, and sent no request to any endpoint that is not a normal page
a reader would open. That restraint is the finding, not a gap in it. Working
out a private API from a site's frontend is not research, and the workspace
`CLAUDE.md` says so.

"They did not document it" is a complete answer.

### 6.3 What FilterBlade does publish about access

The Contact page carries the closest thing to terms of use.

Verbatim, from https://www.filterblade.xyz/Contact:

> The code - while open source - is copyrighted by us (NeverSink, Zoey,
> Haggis). We do not permit sharing, rehosting, redistributing, modifying or
> monetizing it, without our explicit permission.

And:

> block your account from our website, deny you services, block or restrict
> your access and delete, block or modify any or all content you have created
> on the site and any related services for any reason we find necessary

`https://www.filterblade.xyz/robots.txt` is:

```
User-agent: *
Disallow:
```

That is permissive to crawlers and says nothing about API use. A permissive
robots.txt is not a licence and is not consent to call an application endpoint.

The NeverSink PoE2 README describes the auto updater as "heavy on our servers,
so it costs a few bucks", at 3 US dollars a month.

- Source: https://github.com/NeverSinkDev/NeverSink-Filter-for-PoE2

### 6.4 Why the decision went the way it did

Four reasons, in the order that matters.

**One. The feature does not need it.** Filters are text files on the player's
own disk. Switching, explaining, editing and reloading all work offline. There
is no gap to fill.

**Two. There is no contract to depend on.** An endpoint with no published
documentation carries no promise. It can change shape, move, or vanish in any
release, and we would find out from a user whose overlay broke.

**Three. Their published terms require permission we do not have.** They forbid
redistributing or modifying their work without explicit permission, and they
reserve the right to block access for any reason.

**Four. The cost lands on volunteers.** They say automated delivery loads their
servers and they price it at 3 US dollars a month. Sending our users at their
servers for free moves that cost onto people who did not agree to it. Asking
them to carry it, or asking them for an exception, is a request we chose not to
make.

Nothing here is a limitation we regret. It is the shape of the feature.

## 7. Research: the official GGG API and GGG's terms

### 7.1 GGG has item filter endpoints, and they are out of scope

**Recorded as a fact, not as a route we are taking.** These endpoints exist.
This feature does not use them. Two reasons.

- They need an OAuth authorization code flow with PKCE, a client registration
  with GGG, and a browser login. That is a login screen bolted onto a feature
  whose whole point is not leaving the game.
- They only reach filters stored on the player's GGG account. The local design
  reads the player's own directory, which already holds what they use.

The design in sections 8 through 10 needs none of it. Nothing below is a plan.

The official developer API exposes item filters.

- Source: https://www.pathofexile.com/developer/docs/reference
- Source: https://www.pathofexile.com/developer/docs/authorization

| Endpoint | Method | Scope |
|---|---|---|
| `/item-filter` | GET | `account:item_filter` |
| `/item-filter/<id>` | GET | `account:item_filter` |
| `/item-filter` | POST | `account:item_filter` |
| `/item-filter/<id>` | POST | `account:item_filter` |

The `ItemFilter` resource carries `id`, `filter_name`, `realm`, `description`,
`version`, `type`, `public`, `filter` and `validation`. `realm` accepts `poe2`,
so both games are covered. Create and update accept `validate=true`, which
validates the filter against the current game version and returns a validation
object. That is a free syntax checker run by GGG itself.

`account:item_filter` is available to public clients using authorization code
with PKCE and a local redirect URI. Public clients share a rate limit pool with
every other public client.

None of this is used. It is written down so the next reader does not spend a
day rediscovering that the endpoints exist. Server side validation is the one
genuinely nice thing here and we replace it with our own parser, which reports
a bad line without a network round trip and without a login.

### 7.2 Do GGG's terms allow what we plan

Yes, with one line we must not cross.

GGG's stated position, from the terms of use and the support forum:

- Programs that interact with the game client are not allowed.
- Tools that are entirely external, such as ones reading the client log files,
  are fine.
- Input automation must follow the macro rules. A macro must be invoked
  manually by the user. Automatic invocation from timers, from reacting to file
  changes, or from reading the screen is not allowed. One invocation performs
  one action, and sending a single chat message counts as one action.
- GGG will not certify any tool. They state they cannot comment on the legality
  of third party tools and cannot guarantee a tool stays allowed.

- Source: https://www.pathofexile.com/legal/terms-of-use-and-privacy-policy
- Source: https://www.pathofexile.com/forum/view-thread/3584808

Applied to us:

| What we do | Verdict |
|---|---|
| Read a `.filter` file from the player's Documents directory | External file read. Fine. |
| Write a `.filter` file the player asked us to write | External file write. Fine. |
| Send `/itemfilter <name>` on a keypress the player made | One action, manually invoked. This is exactly what the macro rules permit. |
| Send a reload on a timer, or when the file changes, or on zone change | Not allowed. Banned in section 3. |
| Read memory, hook the renderer, inject into the client | Never. Not proposed. |

We found nothing that treats modifying a filter file while the client runs
differently from modifying it while the client is closed. The game reads the
file on reload. It does not hold a lock. The precedent tool
`PoEDynamicLootFilter` has done exactly this for years, writing the player's
filter file and reloading it from a hotkey.

- Source: https://github.com/Apollys/PoEDynamicLootFilter

The rule to enforce in code is the invocation rule. Every reload we send must
be traceable to a keypress. The overlay already sends chat on a keypress and
already has a coalescing rule for double reported presses in
`core::controller::press_coalesce`.

## 8. Where filter files come from

**The overlay never downloads a filter. The player supplies the files. The
overlay finds, reads, explains, edits and reloads them.**

Fetching was the only job that ever wanted a network host, and it is not our
job. Getting a filter file is already a solved, one time, out of game task with
several good routes. Doing it mid map is not the problem players have. The
problem is everything that happens after the file is on disk.

### 8.1 The three cases

**Case one. The player already has filters.** This is almost everyone. Any
player who has ever used a filter has files in
`Documents\My Games\Path of Exile` or `Documents\My Games\Path of Exile 2`. The
overlay lists them and the whole feature works. Where the files came from does
not matter to us.

**Case two. The player downloads a file and drops it in.** They fetch it in a
browser however they like, save it into the filter directory, and press
`Refresh` in the panel. The overlay rescans the directory and the new file
appears in the list. No restart. This is the supported way to add a filter.

**Case three. There is no filter at all.** The panel says so and explains the
fix in one screen:

```
No filters found.

Looked in:
  C:\Users\<you>\Documents\My Games\Path of Exile 2\

Put a .filter file in that folder, then press Refresh.
Path of Exile has an official guide at pathofexile.com/item-filter/about
```

It shows the full path, never truncated, so the player can paste it into
Explorer. It offers no download button and it creates no directory. The game
creates that directory on first run, so a missing directory means the game has
not run yet, and creating it ourselves would hide that.

### 8.2 The strictness ladder still works offline

The common filter sets ship as seven files named `0-SOFT` through
`6-UBER-PLUS-STRICT`. That naming is enough to build an ordered strictness
ladder from a directory listing alone. A player who dropped in the seven file
set once gets `Stricter` and `Softer` forever, with no network.

A player with one file gets everything except the ladder. The panel says
`Only one filter found` rather than showing a dead button.

Files the overlay cannot place in the ladder go in an `Other` list,
alphabetical. Nothing is hidden from the player because we did not recognise
its name.

### 8.3 Sources of data, and what each is allowed to do

| Source | What it gives | Runtime access |
|---|---|---|
| The player's `Documents\My Games\<game>\*.filter` | Everything in section 2 | Read and, on confirm, write. No network. |
| The player's `onlinefilters` subdirectory | Names of account synced filters | Read only, always. Never written. No network. |
| `www.pathofexile.com/item-filter/about` | The condition and action grammar | None. A human read it and encoded it as a table in our source. Not fetched at runtime. |
| `filterblade.xyz` | Nothing | **Never.** Settled, permanent. Section 6. |
| `github.com` | Nothing | **Never.** The overlay does not download filters. |
| `api.pathofexile.com` item filter endpoints | Nothing | **Never in this feature.** Out of scope. Section 7.1. |
| `api.exiledexchange2.dev` | Nothing | **Permanently banned.** Not an option for anything. |

The allowlist stays `www.pathofexile.com`. This feature adds zero outbound
requests. `loot_filter_controller` holds no http adapter, and a test asserts
that.

## 9. Where each piece lives

Strict hexagonal, per the workspace rule. `poe-wayfinder-core` does no I/O.

| Piece | Layer | Where | Why |
|---|---|---|---|
| `.filter` grammar. Text to blocks, blocks to text | core controller | `poe-wayfinder-core/src/controller/loot_filter/parse.rs` | Pure string work. Same shape as `controller/parse/`. Testable with no disk. |
| Filter block and condition types | core types | `poe-wayfinder-core/src/types/loot_filter_types.rs` | Plain data. |
| Match a `ParsedItem` against blocks and report which block won | core controller | `poe-wayfinder-core/src/controller/loot_filter/explain.rs` | Pure. Reuses the existing item parser output, so an item copied for a price check is already in the right shape. |
| Strictness ladder from file names | core controller | `poe-wayfinder-core/src/controller/loot_filter/strictness.rs` | Pure. Names in, ordered list out. |
| Round trip edit. Change one block, keep every byte we did not touch | core controller | `poe-wayfinder-core/src/controller/loot_filter/edit.rs` | Pure. This is the piece that protects hand edits. |
| List, read and write `.filter` files | app adapter | `poe-wayfinder-app/src/adapter/filter_file_adapter.rs` | Disk I/O. Sits beside `game_config_adapter.rs` and reuses its documents path. |
| Orchestration. Which filter is active, apply an edit then reload | app controller | `poe-wayfinder-app/src/controller/loot_filter_controller.rs` | Calls the adapter and the core controllers. Owns nothing else. |
| The panel | app driver | a new `Tab::Filter` in `widgets_controller.rs` and its draw code in `overlay_ui_driver.rs` | Drivers own input and pixels. The tab system already exists with nine tabs. |
| Sending `/itemfilter <name>` | existing | `core::controller::chat::type_in_chat` and `driver/chat_driver.rs` | Already built. No new code beyond calling it. |

Two things the codebase already gives us for free:

- `game_config_adapter.rs` already resolves `Documents\My Games\Path of Exile`
  and `Documents\My Games\Path of Exile 2`. The filter directory is that same
  directory.
- `core::controller::game_detect` already follows the foreground window between
  the two games. The filter panel follows it for free, the way the parser and
  the trade endpoint already do.

New public functions must be reachable from `src/driver/`. The `architecture`
stage fails on any `pub fn` that no production code calls, and the `uiparity`
stage counts an entry only when domain code and a driver symbol both exist.

## 10. Risks

**Parsing a large filter is not free.** A NeverSink STRICT filter is thousands
of lines and tens of thousands of tokens. Parsing it on the UI thread will drop
frames. The parse must happen off the frame loop and the result must be cached
against the file's modified time. The overlay already learned this lesson with
the data refresh, which runs on a background thread and never blocks startup.

**Round trip fidelity is the whole feature's credibility.** If we write a file
back and it differs anywhere we did not intend, we have damaged the player's
filter. The edit model must be byte preserving outside the edited span. The
test that proves it is a property test: parse every filter in a corpus, write
it back unchanged, assert the bytes are identical.

**PoE2 filter support is younger and moves faster.** GGG adds conditions to
PoE2 filters between patches. A parser that rejects an unknown condition will
break on patch day. The parser must accept any unknown line, keep it verbatim,
and mark it unrecognised, rather than fail. Explain output for such a block
says so honestly.

**`Import` makes "the filter" more than one file.** A filter that imports
others must be resolved before matching. An import cycle must be detected. A
missing non `Optional` import is an error the game reports and we must report
the same way.

**`Continue` breaks the "first match wins" mental model.** The explain feature
must report an ordered list of blocks that touched the item, not one block, or
it will lie about styled items.

**`/itemfilter` needs the exact filter name.** A wrong name is a failed switch
and a confusing chat line. The name comes from the file name without the
extension. Local and online filters can share a name, which is the exact
conflict the ChaosRecipeEnhancer issue describes. We show both and say which is
which.

**We cannot read what the game currently has loaded.** No API or file tells us
the active filter. We can read the config ini and we can remember what we last
switched to. Anything else is a guess and must be labelled as one.

**Chat command sending is input automation.** It is allowed only because a
human pressed a key. Any code path that could fire it without a press is a
terms of use risk. Test for it.

## 11. Decisions on the record

All three are closed. None is waiting on anything. A reader six months from now
should treat this section as history, not as a menu.

| Question | Decision | Date | Reason |
|---|---|---|---|
| Does the overlay call filterblade.xyz? | **No. Permanently. We do not ask its maintainer either.** | 2026-08-14 | No documented API. Their terms require explicit permission to redistribute or modify. Every job ships without them. Section 6. |
| Does the overlay download filters from anywhere? | **No.** | 2026-08-14 | Fetching is a one time, out of game task the player already does well. The problem is what happens after the file is on disk. Section 8. |
| Does the overlay use GGG's `/item-filter` endpoints? | **No. Out of scope.** | 2026-08-14 | Needs OAuth with PKCE and a browser login, and only reaches account stored filters. The local design does not need it. Section 7.1. |
| Does the overlay use `api.exiledexchange2.dev`? | **No. Permanently banned.** | Standing workspace rule | The fork maintainer's own server. Never used, never reconsidered. |

What this costs, stated honestly, so nobody reopens it hoping for a win:

- A player with exactly one filter file gets no strictness ladder. They fix it
  by dropping more files in the folder once.
- A player with a stale filter is told it is stale and fixes it in a browser.
- We do not get FilterBlade's tiering model or its auto update.

What it buys:

- The feature works with no network at all, which is what a player mid map
  actually needs.
- Nothing breaks when somebody else's server changes.
- We spend nobody's bandwidth but our own, and we spend none.

If a future feature genuinely needs an outbound host, it gets proposed to the
user by name, on its own merits, with what breaks without it. It does not
inherit approval from this document.
