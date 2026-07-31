# rodeo 🤠

Rodeo is a terminal file manager inspired by Norton and Midnight Commander,
written in Rust. It pairs the classic dual-pane layout with Vim-style
keybindings, a rich preview, and themes — with no runtime dependencies beyond a
terminal.

> **Status:** pre-release (0.1.0). Everything below works today; see
> [TODO.md](TODO.md) for what is left before a tagged release.

## Features

- **Dual-pane navigation** with per-pane sorting, filtering and hidden-file
  toggling; the active pane is highlighted and the inactive one dimmed.
- **File operations** — copy, move, rename, mkdir, touch, delete to trash
  (with a permanent-delete fallback), on the selection or the highlighted
  entry. Large transfers run in the background with a progress gauge and can
  be cancelled.
- **Preview** (`Space`) — syntax highlighting via syntect, images, archive
  listings (zip/tar/tar.gz), PDF text, directory sizes, and hex dumps with
  metadata for binaries. Slow content loads on a worker thread behind a
  spinner; long lines wrap (`w` toggles).
- **Search** — fuzzy find (`/`), regex filter (`Ctrl+f`) and recursive
  find-in-files (`Ctrl+g`, honours `.gitignore`), with matches highlighted in
  the listing.
- **Git aware** — entry names are coloured by status, the header shows the
  branch and counts, and wide terminals get a status column showing the raw
  porcelain code (so staged and unstaged changes are distinguishable).
- **Power tools** — bulk rename with regex or numbering (`B`), a trash browser
  (`:trash`), on-demand directory sizes (`S`), wildcard selection (`*`), and a
  command palette (`:`) including shell commands.
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

`~/.config/rodeo/config.toml`, created with defaults on first run.

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

# Optional: remap any single-key action. Unlisted actions keep their defaults.
[keybindings]
quit = "Q"
preview = "v"
```

`:so` reloads the config at runtime, `:w` writes the current settings back.

Bundled themes: `default`, `catppuccin-frappe`, `catppuccin-latte`,
`catppuccin-macchiato`, `catppuccin-mocha`, `dracula`, `github-dark`, `nord`,
`solarized-dark`, `tokyo-night`. Switch at runtime with `:theme <name>`.

## Keybindings

`F1` shows this list in the app.

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
| `r`, `F2` | Rename |
| `B` | Bulk rename (2+ selected) |
| `F5` / `F6` | Copy / move to the other pane |
| `F7` / `Ctrl+t` / `a` | Create directory / empty file / file-or-dir |
| `dd`, `Del`, `F8` | Move to trash |
| `/`, `F3` | Fuzzy search |
| `Ctrl+f` / `Ctrl+g` | Regex filter / find in files |
| `S` | Compute directory sizes |
| `Shift+←/→` / `Shift+O` | Change sort column / reverse order |
| `Ctrl+h` / `Ctrl+l` | Toggle hidden files / refresh |
| `:` | Command palette |
| `?` / `F1` | About / help |
| `Esc` | Close, clear filter, clear selection, then quit |
| `q`, `F10` | Quit |

### Commands

`:q` `:w` `:so` `:e <path>` `:mkdir` `:touch` `:delete` `:rename <new>`
`:theme [name]` `:trash` `:shell` `:!<cmd>` `:help`

`:!` output opens in the preview popup, and `%f` expands to the selected (or
highlighted) paths — `:!wc -l %f`.

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

Not yet chosen — the repository currently ships without a licence file, which
means default copyright (all rights reserved). Pick one before publishing a
release.
