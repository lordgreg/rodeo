# Unreleased

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
* The header's git branch and counts come from the listing's own `git status`
  run. Opening a directory used to start seven `git` processes on the UI
  thread; it now starts four.
* `Esc` no longer quits. It closes a popup, then clears the filter, then
  clears the selection, and does nothing once there is nothing left to back
  out of — a reflexive second press used to exit rodeo. Quitting is `q`
  or `:q`.
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
