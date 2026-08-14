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
7. Add zero new outbound hosts. The default build must ship with the allowlist
   unchanged.

## 3. Non goals

- No FilterBlade integration in this scope. It stays a live option the user can
  approve later. Section 6 and section 11 lay out what it would take.
- No filter authoring from scratch. We edit and explain filters the player
  already has. We do not become a filter generator.
- No automatic download of NeverSink filters in the default build. That is the
  optional item in section 8.
- No sound or visual preview of alert sounds and beams. We report what a block
  says, we do not render the game's presentation.
- No writing to online filters stored on the GGG account. Local files only.
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

The seven level naming is a gift. It gives us an ordered strictness ladder
purely from file names in the player's own directory. No network call is
required to offer "one step stricter" when the player already has more than one
NeverSink file on disk. That is the common case, because FilterBlade and the
GitHub download both write files named this way.

## 6. Research: FilterBlade

### 6.1 What it is

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
a reader would open. That restraint is the finding, not a gap in it. Section
6.4 explains.

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

### 6.4 What calling FilterBlade would cost

FilterBlade is not banned. It is unapproved. Four costs decide whether it is
worth approving.

**One. There is no contract to depend on.** An endpoint with no published
documentation carries no promise. It can change shape, move, or vanish in any
release, and we would find out from a user whose overlay broke.

**Two. The maintainers charge for exactly this.** They say automated delivery
loads their servers and they price it at 3 US dollars a month. Sending our
users at their servers for free moves that cost onto volunteers who did not
agree to it.

**Three. Their published terms point the other way.** They forbid
redistributing or modifying their code without explicit permission, and they
reserve the right to block access for any reason. Asking first turns that from
a risk into an agreement.

**Four. We would have to guess the endpoints.** No documentation exists, and
probing or reading their bundles is off limits. That leaves one honest route.
Ask the maintainer.

The good news is the size of the remainder. Almost everything the player wants
sits in files on their own disk. Section 11 sizes what is left.

`api.exiledexchange2.dev` is a separate matter. It is permanently banned and is
not an option for anything.

## 7. Research: the official GGG API and GGG's terms

### 7.1 GGG has item filter endpoints

The official developer API exposes item filters. This is the one legitimate
network path that exists.

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

This costs an OAuth client registration with GGG, a browser login flow, and a
new allowlisted host. It buys account synced filters and server side
validation. It is not needed for anything in section 2. Treat it as a later
option, not part of this scope.

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

## 8. External data sources and the allowlist

| Source | What it gives | Licence | Allowed under current rules |
|---|---|---|---|
| The player's own `Documents\My Games\<game>\*.filter` | Everything in section 2 | The player's own files | Yes. No network. |
| The player's `onlinefilters` subdirectory | Names of account synced filters, read only | The player's own files | Yes. No network. |
| `www.pathofexile.com/item-filter/about` | The condition and action grammar | GGG documentation, read by a human, encoded as a table in our source | Yes. Already the only allowed host, and we read it at development time, not at runtime. |
| `github.com` NeverSink releases | Fresh filter files, seven strictness levels | MIT | **No today.** Not on the allowlist. Public, documented, MIT, and a stable release URL, so it is a defensible addition. Needs the user's explicit approval. |
| `api.pathofexile.com` item filter endpoints | Account synced filters and GGG side validation | GGG official API, public client with PKCE | **No today.** Not on the allowlist. Needs an OAuth client registration and the user's explicit approval. |
| `filterblade.xyz` | Its editor model, tiering, and auto update | Explicitly forbids redistribution and modification without permission. No documented API. | **No today. Not banned.** Live option. Needs the user's explicit approval by host name, and needs an answer from the maintainer first. See section 6.4 and 11. |
| `api.exiledexchange2.dev` | Nothing we want | The fork maintainer's own server | **Permanently banned.** Never an option. |

The default build adds nothing to the allowlist. Everything in section 2 works
offline. Only the user adds a host, by name, case by case.

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

## 11. The decisions for the user

Everything in section 2 ships with no third party and no allowlist change.
Build that first and ship it. Nothing below blocks that work.

Only the user adds a host to the allowlist, by name, case by case. Two hosts
are on the table.

### 11.1 filterblade.xyz

The user said "we can probably use filterblade". Here is what that would take.

| Route | What happens | Cost | Risk |
|---|---|---|---|
| **Approve the host now** | We add `filterblade.xyz` and build against endpoints we infer from public pages only | Fast | High. No documented contract. Their terms forbid modification and reserve the right to block us. We would be spending a volunteer's server budget without asking. |
| **Ask the maintainer first** | Open a GitHub issue or use the Contact page. Ask for a documented endpoint and permission for a free desktop overlay to call it | One message and a wait | Low. A yes gives us a contract. A no costs nothing, because section 2 already shipped. |
| **Stay local only** | Ship section 2 and stop | None | None. Loses the FilterBlade tiering model and auto update. |

**Recommendation: ask the maintainer first.** Ship section 2 while waiting. It
costs one message. It turns an undocumented dependency into an agreement, and
it is the only route that respects the terms they published. Approving the host
before asking buys speed we do not need, since the offline feature already
covers goals 1 through 7.

### 11.2 github.com

**Should the overlay download NeverSink filters from GitHub Releases?**

| | Ship without it | Add `github.com` to the allowlist |
|---|---|---|
| Player already has filters on disk | Full feature. Switch, explain, edit, reload. | Same |
| Player has only one filter file | Can explain and edit it. Cannot offer a strictness ladder. | Can fetch all seven levels and offer the ladder |
| Player's filter is a league out of date | We can say so from the file's date. We cannot fix it. | One click to fetch the current one |
| Allowlist | Unchanged. `www.pathofexile.com` only. | One new host |
| Licence risk | None | None. MIT. |
| Dependency risk | None | GitHub Releases is a documented, stable, public URL scheme run by Microsoft, not a volunteer's server |

**Recommendation: ship without it, then add it behind a setting that defaults
off.** The offline feature is the large majority of the value and it carries no
risk at all. GitHub is a documented, stable, public, MIT licensed source and it
runs on Microsoft's machines rather than a volunteer's. Put it behind a setting
that defaults off, so the default build still adds no host. Let the player turn
it on knowing what it does.

### 11.3 api.pathofexile.com

A third option and it should wait. It buys account synced filters and GGG side
validation for the price of an OAuth client registration and a browser login
flow. Revisit once the offline feature has users.
