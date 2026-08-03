# Unreleased

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
