# Unreleased

## Fixed

* `--config` now holds for the whole session. `:w` wrote to
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

* **Bulk rename moved from `B` to `R`.** `b` and `B` are now bookmarks, which
  only works as a pair. `[keybindings] "B" = "bulk_rename"` in `config.toml`
  restores the old key.

## Added

* **Bookmarks.** `b` bookmarks the entry under the cursor — or every marked
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
* The filter bar picks the query out from its label: the pattern is drawn in
  the same colour that marks its matches in the listing, so an active filter
  reads at a glance rather than as one uniform grey line.

# 0.2.0

## Fixed

* A theme file with a malformed colour no longer crashes rodeo mid-frame and
  leaves the terminal unusable. Colours are checked when the theme loads, the
  offending key is named, and rodeo starts on the built-in theme instead.
  Three-digit colours (`#fff`) are now accepted.
* The start directory is read from `$HOME` when rodeo runs, not when it was
  built. Packaged builds used to start every user in the build machine's home
  directory; an existing `config.toml` carrying such a path is repaired on
  load.
* The hint bar shows the keys that are actually bound. Bindings using `alt+`
  or `shift+` were printed literally (`alt+j Copy`), and the bar shown while
  entries are selected ignored the keymap entirely, always claiming `F5` /
  `F6` / `F8`.
* Opening a directory no longer waits for git. The branch, the counts and the
  status column all come from one `git status` per pane, and that runs on a
  background thread: the listing appears at once and the status column fills
  in a moment later. Navigating used to start seven `git` processes on the UI
  thread and block on all of them, which was felt on any large repository.
* Previewing a very large file no longer freezes rodeo. The preview reads the
  lines it needs and stops, capped at 8 MiB, instead of loading the whole file
  into memory — a multi-gigabyte log, or a binary containing no newline at
  all, used to be read in full.
* Image previews are decoded once instead of once per frame. Every redraw used
  to re-read the file from disk, decode it again, and re-probe the terminal
  for its graphics support while the frame was being painted.
* Directory listings do less work per entry: one `stat` call for an ordinary
  file or directory where there were three, and three for a symlink where
  there were six. Large directories open noticeably faster.
* `Esc` no longer quits. It closes a popup, then clears the filter, then
  clears the selection, and does nothing once there is nothing left to back
  out of — a reflexive second press used to exit rodeo. Quitting is `q`
  or `:q`.
* The `accent2` and `accent3` theme colours were unreachable: every bundled
  theme defines them, but no widget could read them. They now have accessors
  like the rest of the palette.
* Pressing `?` with the file preview open drew the help table and the preview
  on top of one another. Opening one popup now closes whatever was already
  there, and a second `Space` closes the preview instead of rebuilding it.

## Added

* `/` now opens a **file finder** popup: every file and directory below the
  active pane, searched by name as you type, with a preview beside the results.
  `Enter` takes the pane to the match, `Ctrl+e` opens it in `$EDITOR`.
* One query language everywhere: a plain word is matched fuzzily, a regular
  expression is matched as one. The pane filter (`Ctrl+f`) no longer has a
  separate regex mode, and the popups say which reading is in force.
* New config keys controlling what the searches may look at, shown along the
  bottom border of both search popups:
  * `filter_gitignore` (default `true`) — obey `.gitignore` / `.ignore`.
  * `filter_hidden` (default `true`) — skip dot-files and dot-directories.
  * `filter_entries` (default `[]`) — extra names to skip: `target`, `*.lock`
    or `src/generated`.
* Find-in-files honours the same filter instead of its own hard-coded rules.

# 0.1.0

* Initial commit, all wanted features implemented.
