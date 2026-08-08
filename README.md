# rodeo 🤠

Rodeo is a terminal file manager inspired by Norton and Midnight Commander,
written in Rust. It pairs the classic dual-pane layout with Vim-style
keybindings, a rich preview, and themes — with no runtime dependencies beyond a
terminal.

## Features

- **Dual-pane navigation** with per-pane sorting, filtering and hidden-file
  toggling; the active pane is highlighted and the inactive one dimmed.
- **File operations** — copy, move, rename, mkdir, touch, delete to trash
  (with a permanent-delete fallback), on the selection or the highlighted
  entry. Large transfers run in the background with a progress gauge and can
  be cancelled.
- **Preview** (`Space` or `F3`) — syntax highlighting via syntect, images, archive
  listings (zip/tar/tar.gz), PDF text, directory sizes, and hex dumps with
  metadata for binaries. Slow content loads on a worker thread behind a
  spinner; long lines wrap (`w` toggles).
- **Search** — one query language everywhere: type a plain word and it is
  matched fuzzily, type a regular expression and it is matched as one. No mode
  to pick first.
  - `/` opens the **file finder**: every file and directory below the active
    pane, searched by name as you type. `Enter` takes the pane to the match
    (into a directory, or onto the file with its parent listed), `Ctrl+e`
    opens it in `editor`.
  - `Ctrl+f` **filters the pane** listing in place, with matches highlighted.
  - `Ctrl+g` is **find-in-files**: greps file contents below the active pane —
    type a regex, `Enter` runs it, `Enter` on a hit opens that file in `editor`
    at the matching line.

  Both popups are split Telescope-style — results on the left, a syntax
  highlighted preview on the right (`Ctrl+n`/`Ctrl+p` move the selection,
  `Ctrl+d`/`Ctrl+u` scroll the preview) — and both obey the search filter
  below, which they name along their bottom border.
- **Git aware** — entry names are coloured by status, the header shows the
  branch and counts, and wide terminals get a status column showing the raw
  porcelain code (so staged and unstaged changes are distinguishable).
- **Bookmarks** — `b` bookmarks the entry under the cursor (or the pane's own
  directory, when the cursor is on `..`), `B` lists them. `Enter` or `1`–`9`
  jumps, `d` removes, and entries whose target has since disappeared are
  flagged `(missing)` so they can be pruned with `P`. One that merely cannot
  be read right now — an unreadable parent, a stalled network mount — is
  flagged `(unreadable)` instead and is never pruned.
- **Power tools** — bulk rename with regex or numbering (`R`), a trash browser
  (`:trash`), on-demand directory sizes (`S`), wildcard selection (`*`), and a
  command palette (`:`) that can capture command output or hand over the
  terminal.
- **Live refresh** — both panes follow filesystem changes automatically.
- **Themes** — ten bundled palettes; syntax colours are derived from the
  active theme.

## Installation

Requires a Rust toolchain (edition 2024, so **Rust 1.85+**; developed on 1.95).

```sh
git clone https://codeberg.org/grepx/rodeo
cd rodeo
cargo install --path .
```

Themes are looked up in, first match wins:

1. `$XDG_DATA_HOME/rodeo/themes` (usually `~/.local/share/rodeo/themes`)
2. `$XDG_DATA_DIRS/rodeo/themes`, e.g. `/usr/share/rodeo/themes`
3. `./themes`, for running from a checkout

So after installing, copy them where rodeo can find them:

```sh
mkdir -p ~/.local/share/rodeo/themes
cp themes/*.toml ~/.local/share/rodeo/themes/
```

Rodeo starts even with no themes installed — it falls back to a compiled-in
copy of the default theme.

A man page is checked in at `docs/rodeo.1`:

```sh
sudo install -Dm644 docs/rodeo.1 /usr/local/share/man/man1/rodeo.1
```

### Usage

```
rodeo [--left <PATH>] [--right <PATH>] [--theme <NAME>] [--config <FILE>]
```

`rodeo --help` lists the flags and where configuration and themes are read
from.

## Configuration

`~/.config/rodeo/config.toml`, created with defaults on first run. `--config
<FILE>` uses that file instead, for the whole session: `:w` writes it, `:so`
re-reads it, and it is created with defaults if it is not there yet.

```toml
theme = "catppuccin-macchiato"      # name in the themes directory, or a path to a .toml
initial_directory_left = "/home/you"
initial_directory_right = "/home/you"
sort_type = "Name"                  # Name | Size | Time | Flagged
sort_order = "Ascending"            # Ascending | Descending
show_hidden = false
directories_on_top = true
active_pane = "Left"                # Left | Right
editor = "nvim"                     # defaults to $VISUAL, then $EDITOR, then vi
icons = false                       # file-type glyphs; needs a Nerd Font

# What `/` (find files) and `Ctrl+g` (find in files) are allowed to look at.
filter_gitignore = true             # skip whatever .gitignore/.ignore exclude
filter_hidden = true                # skip dot-files and dot-directories
filter_entries = ["target", "node_modules", "*.lock", "src/generated"]
                                    # extra names: a name, *.ext, or a sub-path

# Optional. The key is on the left, what it does on the right: either an
# action name, a `:command`, or "none" to free the key.
[keybindings]
"Q" = "quit"                 # add a key for a built-in action
"ctrl+r" = "refresh"         # modifiers work: ctrl+, alt+, shift+
"z" = ":term lazygit"        # run a command, exactly as if typed after `:`
"f9" = ":!git status --short"
"q" = "none"                 # free a key
```

Overriding a key rodeo already uses is allowed, but it says so on startup —
and warns loudly if an action is left with no key at all. `:so` reloads the
bindings without restarting.

Action names: `open` `parent` `first` `last` `select`
`select_all` `glob` `sizes` `quit` `left` `right` `switch` `help`
`preview` `search` `filter` `find` `palette` `rename` `create` `yank` `paste`
`paste_move` `delete_chord` `copy` `move` `delete` `down` `up` `hidden`
`refresh` `sort_next` `sort_prev` `sort_reverse` `bulk_rename` `bookmark`
`bookmarks`

`:so` reloads the config at runtime, `:w` writes the current settings back.

### Bookmarks

Bookmarks live in `bookmarks.toml` beside `config.toml` (so `--config
./rodeo.toml` keeps its own set next to it), as a plain list of paths:

```toml
paths = ["/home/you/src/rodeo", "/etc/nginx/nginx.conf"]
```

They are written the moment one changes, not on `:w` — `config.toml` is yours
to edit, and folding machine-managed state into it would rewrite your settings
on every keypress. A missing or malformed `bookmarks.toml` starts an empty
list with a warning rather than refusing to start.

Bundled themes: `default`, `catppuccin-frappe`, `catppuccin-latte`,
`catppuccin-macchiato`, `catppuccin-mocha`, `dracula`, `github-dark`, `nord`,
`solarized-dark`, `tokyo-night`. Switch at runtime with `:theme <name>`.

## Keybindings

The defaults are vim-first: no function keys, one key per job. `?` shows this
list in the app, and the bar along the bottom always shows the keys that are
actually bound — rebind something and the bar says so.

| Key | Action |
|-----|--------|
| `j` `k` / `↓` `↑` | Move cursor |
| `g` / `G` | First / last entry |
| `h` / `l` / `Tab` | Left pane / right pane / switch |
| `Enter` | Open directory, or edit file in `$EDITOR` |
| `Backspace` | Parent directory |
| `Space` | Preview (`w` wraps, `Ctrl+f/b` page, `Ctrl+d/u` half-page, `Ctrl+j/k` scroll) |
| `x` / `*` / `Ctrl+a` | Toggle selection / select by wildcard / select all |
| `y` / `p` / `P` | Yank / paste copy / paste move |
| `Y` / `M` | Copy / move to the other pane (one-key `y`+`Tab`+`p`) |
| `r` | Rename |
| `R` | Bulk rename (2+ selected) |
| `b` / `B` | Bookmark the entry (or the pane's directory on `..`) / list bookmarks |
| `a` | Create file, or directory with a `/` suffix |
| `dd`, `Del` | Move to trash |
| `/` | Find files by name (fuzzy or regex) |
| `Ctrl+f` / `Ctrl+g` | Filter the pane / find in files |
| `S` | Compute directory sizes |
| `Shift+←/→` / `Shift+O` | Change sort column / reverse order |
| `Ctrl+h` / `Ctrl+l` | Toggle hidden files / refresh |
| `:` | Command palette |
| `?` | Help (and the version, on the bottom border) |
| `Esc` | Close popup, clear filter, then clear selection |
| `q` | Quit |

### Midnight Commander keys

Function keys are not bound by default: they duplicate keys that already
exist, and terminals steal several of them (`F10` opens the menu in GNOME
Terminal, `F1` opens help in others). If you want them anyway, paste this into
`config.toml` — `:so` applies it without a restart, and the footer relabels
itself to match:

```toml
[keybindings]
f1 = "help"
f2 = "rename"
f3 = "preview"
f4 = "open"
f5 = "copy"
f6 = "move"
f7 = "create"   # a directory needs the `/` suffix; `:mkdir <name>` is the direct route
f8 = "delete"
f10 = "quit"
```

### Commands

`:` opens the command line. Matching commands are listed as you type, with
their arguments and a description; `Tab` walks the list and `Shift+Tab` goes
back, Vim-wildmenu style. Arguments complete too — directories for `:e`/`:cd`,
theme names for `:theme`.

| Command | Action |
|---------|--------|
| `:q` `:quit` | Quit |
| `:w` `:write` | Save the configuration |
| `:so` `:source` | Reload the configuration |
| `:e` `:cd <path>` | Navigate to a directory |
| `:mkdir <name>` | Create a directory |
| `:touch <name>` | Create an empty file |
| `:rename <new>` | Rename the current entry |
| `:delete` | Trash the selected or current entries |
| `:theme [name]` | Switch theme, or list the available ones |
| `:trash` | Browse the trash |
| `:bookmarks` | Browse the bookmarks |
| `:term <cmd>` | Run a command attached to the terminal (lazygit, htop…) |
| `:!<cmd>` | Run a shell command and show its output |
| `:help` | Show the help popup |

`%f` expands to the selected (or highlighted) paths in both `:!` and `:term` —
`:!wc -l %f`, `:term nvim %f`.

The two differ in who owns the terminal:

- **`:!<cmd>`** captures stdout and stderr and shows them in the scrollable
  preview popup. Use it for output you want to read: `:!git log --oneline`.
- **`:term <cmd>`** hands the terminal to the program — rodeo leaves the
  alternate screen, runs it attached, and comes back when it exits. Use it for
  anything interactive: `:term lazygit`, `:term htop`, `:term $SHELL`.

Interactive programs open `/dev/tty` directly rather than writing to stdout, so
running one under `:!` produces no capturable output: it will work, but rodeo
cannot show you anything afterwards and says so in the footer. `:term` is the
one to reach for.

## Releases

Pushing a `v*` tag builds and publishes a Linux x86_64 archive:

```sh
git tag -a v0.1.0 -m "rodeo 0.1.0"
git push origin v0.1.0
```

`.forgejo/workflows/release.yml` runs on Codeberg (needs *Settings → Units →
Actions* enabled); `.github/workflows/release.yml` does the same on a GitHub
mirror. The archive carries the binary, the themes, the man page, the README
and the licence, plus a `.sha256`.

## Dependency updates

`renovate.json` drives a self-hosted [Renovate](https://docs.renovatebot.com/)
that runs daily from `.forgejo/workflows/renovate.yml`, or on demand from the
Actions tab (with optional dry-run and debug logging). It needs a
`RENOVATE_TOKEN` secret — see the header of that workflow.

Patch and minor crate updates are grouped into one pull request, major ones
arrive separately, and ratatui and its ecosystem move together because they do
not compile apart. `cargo deny check` in CI is the other half of this: Renovate
proposes newer versions, cargo-deny fails the build on advisories.

## Development

```sh
cargo test                        # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo deny check                  # licences and security advisories
cargo run --example gen_man       # regenerate docs/rodeo.1 after CLI changes
```

The crate is a library (`src/lib.rs`) with a thin binary, so integration tests
can reach the internals.

## License

Licensed under the Apache License, Version 2.0 — see [LICENSE](LICENSE).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in rodeo shall be licensed as above, without any additional terms
or conditions.
