# Unreleased

## Fixed

- Fix wrong brew formula name as const

# 0.4.2

## Updated

- Updater will immediatelly apply update if it finds new version and prompt
  the user to restart the client.

# 0.4.1

## Updated

- MacOS brew version check checks again brew json output and validates the
  returned object's version with the current. This way we always get version
  information back. Oh, I've added tests to, which is crazy!

# 0.4.0

## Added

- **Updater** Based on the system your are currently executing from, updater
  will try to fetch the last version from official github repo. The update
  should complete without the user noticing or waiting. If you wish to disable
  the updater, you can set the `auto_update` to false in config.toml.

## Changed

- **Installer** on linux, the default prefix is $HOME/.local/bin. No need to
  mess with sudo.

# 0.3.2

## Fixed

- **Themes are found on a Homebrew install.** `brew install lordgreg/rodeo/rodeo`
  only ever offered the compiled-in `default` theme — the release archive
  carries the full set, but nothing installed them anywhere rodeo looked, and
  the lookup itself only knew `$XDG_DATA_HOME`/`$XDG_DATA_DIRS`, neither of
  which covers Homebrew's prefix on macOS. The formula now installs `themes/`
  into `pkgshare`, and the search path gained two entries relative to the
  running binary's own directory: `<bin>/../share/rodeo/themes` (the
  `bin`/`share` layout Homebrew and most package managers install into) and
  `<bin>/themes` (the release archive's own layout, for running straight out
  of an extracted tarball). A user's own `$XDG_DATA_HOME/rodeo/themes` is
  still searched first and still the only place a custom theme belongs.

# 0.3.1

## Fixed

- Two bookmark tests failed on macOS (`aarch64-apple-darwin`): a bookmark is
  stored canonicalized (so the same directory reached two ways is one
  bookmark), but the tests compared it against the tempdir's raw path.
  macOS temp directories sit under `/var`, itself a symlink to
  `/private/var`, so the two forms differed there and matched by coincidence
  everywhere else. The tests now canonicalize the path they compare against,
  the way the rest of the suite already does. No application behaviour
  changed.

# 0.3.0

## Fixed

- **A move within one filesystem is a rename again.** Anything over 10 MiB went
  to the background worker, which only ever copied byte-for-byte and then
  deleted — a 32 GiB tree took half a minute instead of one syscall. Moves now
  try `rename` first, and only what it cannot take (a cross-device move, a
  directory to merge into) is sized up and copied. The size walk that feeds the
  progress gauge also ran twice; it runs once, and not at all when nothing is
  left to copy. A `rename` failing for any other reason is now reported rather
  than silently retried as a copy.

- **Git status no longer comes back empty for a pane reached through a
  symlink.** `git rev-parse --show-toplevel` resolves symlinks before
  reporting the repository root, but the status paths built from it were
  then matched against the pane's own, unresolved path — a mismatch that
  silently produced no results. macOS hits this on every session, since its
  temp directories sit under `/var`, itself a symlink to `/private/var`.
  Status paths are now matched against a resolved copy of the pane
  directory, while the file listing keeps using the original path.

- `--config` now holds for the whole session. `:w` wrote to
  `~/.config/rodeo/config.toml` whatever file had been loaded, so a session
  started with `--config ./rodeo.toml` silently overwrote the user's real
  configuration; `:so` read that same wrong file back; and a `--config`
  pointing at a file that did not exist yet created the default in the user's
  configuration directory and left the named one missing. All three resolved
  the default location afresh instead of using the path the session was
  started with. `Config::save_config` takes the path to write rather than an
  `Option` that falls back to the default, so the fallback can no longer be
  reached by accident.

## Changed

- **No more `x86_64-apple-darwin` release archive.** Intel Macs have to build
  from source. `cargo-deny` and the README follow the same list.

- **Bulk rename moved from `B` to `R`.** `b` and `B` are now bookmarks, which
  only works as a pair. `[keybindings] "B" = "bulk_rename"` in `config.toml`
  restores the old key.

## Added

- **Directory tree panel.** `t` switches a pane from the flat listing to a
  collapsible tree rooted at the directory it was already showing, for finding
  your way around a deep project without walking into it one `Enter` at a time.
  `Enter` and `Right`/`Left` open and close a directory in place; `Right` on an
  one already open steps onto its first child, `Left` on something with nothing
  to close steps back out to the parent row. `Backspace` re-roots one level up
  and leaves the directory just left open, so the rows the cursor came from do
  not vanish. Only directories that have actually been opened are read, one
  level at a time — a tree costs one `read_dir` per open node, never a
  recursive walk — and which ones are open is remembered by path, so a
  filesystem event rebuilding the listing does not collapse everything.

  It is a full listing, not a navigator: copy, move, delete, rename and the
  rest work from a tree row as they do from a flat one. Two things follow from
  that. Operations that used to read the pane's own directory now ask where the
  _cursor_ is, since in a tree those are not the same thing and a paste would
  otherwise land somewhere the user never pointed at. And a copy or move
  recreates the layout the sources were listed under: selecting `src/config.rs`
  and `tests/config.rs` together and flattening both into the destination would
  have let the second silently overwrite the first. In a flat listing every
  source is a direct child, so that reconstruction is a no-op and nothing
  changes.

  A filter (`/`, `Ctrl+f`) prunes a tree rather than ranking it, keeping every
  directory on the way down to a match: ordering the rows best-first would tear
  children away from their parents and leave the indentation describing a shape
  that is no longer on screen. The filesystem watcher follows each open node
  separately — it is deliberately not recursive, since watching a root like
  `$HOME` recursively would mean walking everything under it whether it is open
  or not.

  Not available inside an archive, whose listing is not a real directory tree;
  `t` there says so rather than doing nothing.

- **Bookmarks.** `b` bookmarks the entry under the cursor — or every marked
  entry at once, like copy and move do, or the pane's own directory when the
  cursor is on `..`. `B` (or `:bookmarks`) lists them: `Enter` and `1`–`9`
  jump, `d` removes, `P` drops the dead ones. A bookmark whose target has
  since been moved or deleted is shown as `(missing)` and refuses to jump,
  rather than silently doing nothing; one that merely cannot be read right now
  is `(unreadable)` and is never pruned, because "I could not look" is not
  "it is gone". Paths are stored absolute and link-free, so the same directory
  reached two ways is one bookmark.

  They are kept in `bookmarks.toml` beside `config.toml`, written the moment
  one changes rather than on `:w`: `config.toml` is a file you edit by hand,
  and rewriting it on every keypress is not a thing a file manager should do.
  The write goes through a temporary file and a rename, so an interrupted one
  cannot cost the whole list, and a corrupt file starts empty with a warning
  instead of stopping rodeo from starting.

- The filter bar picks the query out from its label: the pattern is drawn in
  the same colour that marks its matches in the listing, so an active filter
  reads at a glance rather than as one uniform grey line.
- **Homebrew.** `brew install lordgreg/rodeo/rodeo` installs a prebuilt
  binary on Apple Silicon macOS from the
  [`lordgreg/homebrew-rodeo`](https://github.com/lordgreg/homebrew-rodeo)
  tap, kept up to date automatically on every release.

# 0.2.0

## Fixed

- A theme file with a malformed colour no longer crashes rodeo mid-frame and
  leaves the terminal unusable. Colours are checked when the theme loads, the
  offending key is named, and rodeo starts on the built-in theme instead.
  Three-digit colours (`#fff`) are now accepted.
- The start directory is read from `$HOME` when rodeo runs, not when it was
  built. Packaged builds used to start every user in the build machine's home
  directory; an existing `config.toml` carrying such a path is repaired on
  load.
- The hint bar shows the keys that are actually bound. Bindings using `alt+`
  or `shift+` were printed literally (`alt+j Copy`), and the bar shown while
  entries are selected ignored the keymap entirely, always claiming `F5` /
  `F6` / `F8`.
- Opening a directory no longer waits for git. The branch, the counts and the
  status column all come from one `git status` per pane, and that runs on a
  background thread: the listing appears at once and the status column fills
  in a moment later. Navigating used to start seven `git` processes on the UI
  thread and block on all of them, which was felt on any large repository.
- Previewing a very large file no longer freezes rodeo. The preview reads the
  lines it needs and stops, capped at 8 MiB, instead of loading the whole file
  into memory — a multi-gigabyte log, or a binary containing no newline at
  all, used to be read in full.
- Image previews are decoded once instead of once per frame. Every redraw used
  to re-read the file from disk, decode it again, and re-probe the terminal
  for its graphics support while the frame was being painted.
- Directory listings do less work per entry: one `stat` call for an ordinary
  file or directory where there were three, and three for a symlink where
  there were six. Large directories open noticeably faster.
- `Esc` no longer quits. It closes a popup, then clears the filter, then
  clears the selection, and does nothing once there is nothing left to back
  out of — a reflexive second press used to exit rodeo. Quitting is `q`
  or `:q`.
- The `accent2` and `accent3` theme colours were unreachable: every bundled
  theme defines them, but no widget could read them. They now have accessors
  like the rest of the palette.
- Pressing `?` with the file preview open drew the help table and the preview
  on top of one another. Opening one popup now closes whatever was already
  there, and a second `Space` closes the preview instead of rebuilding it.

## Added

- `/` now opens a **file finder** popup: every file and directory below the
  active pane, searched by name as you type, with a preview beside the results.
  `Enter` takes the pane to the match, `Ctrl+e` opens it in `$EDITOR`.
- One query language everywhere: a plain word is matched fuzzily, a regular
  expression is matched as one. The pane filter (`Ctrl+f`) no longer has a
  separate regex mode, and the popups say which reading is in force.
- New config keys controlling what the searches may look at, shown along the
  bottom border of both search popups:
  - `filter_gitignore` (default `true`) — obey `.gitignore` / `.ignore`.
  - `filter_hidden` (default `true`) — skip dot-files and dot-directories.
  - `filter_entries` (default `[]`) — extra names to skip: `target`, `*.lock`
    or `src/generated`.
- Find-in-files honours the same filter instead of its own hard-coded rules.

# 0.1.0

- Initial commit, all wanted features implemented.
