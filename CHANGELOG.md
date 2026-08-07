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
