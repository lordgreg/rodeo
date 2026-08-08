# TODO

## Priority Items

Nothing outstanding. The seven items that stood here were closed for 0.2.0 —
see below for what each turned out to be.

---

## Feature Ideas

Not scheduled, not designed — a candidate list for where rodeo could go next,
ranked roughly by fit/impact. Kept lean and MC-inspired: prefer extending
existing subsystems and shelling out to external tools over adding heavy
runtime dependencies or a scripting layer.

1. **Archive creation.** The inverse of the archive VFS below — pack a
   pane's selection into a zip/tar.gz. Same deps (`zip`, `tar`, `flate2`),
   closes the asymmetry.
2. **Directory compare & sync.** Diff two dirs (name/size/mtime, optionally
   content hash), highlight differences pane-to-pane, offer bulk
   copy/sync — a classic MC feature that maps directly onto the dual-pane
   model and the existing diff-coloring used for git status.
3. **Undo/redo for destructive ops.** An in-session journal of the last N
   copy/move/delete/rename operations with `u` to reverse. Delete already
   routes through `trash`, so this is mostly bookkeeping plus reversing
   copy/move/rename.
4. **Permission/ownership editor + symlink creation.** A small popup (like
   `popup_bulkrename.rs`) for chmod (octal + rwx toggles) and chown, plus a
   "create symlink" command. No new deps — `libc` is already pulled in.
5. **Duplicate-file / hash finder.** Background-thread scan (same spawn
   pattern as git status / `notify`) that hashes files under a pane and
   flags content duplicates, not just name matches.
6. **Per-extension user actions.** Config-driven "open with," e.g. an
   `[actions]` table mapping globs to shell commands (`*.pdf = "zathura %f"`).
   Reuses the `%f` expansion already implemented for `:!`/`:term`.
7. **Directory tree panel.** A collapsible tree view as an alternative to the
   flat listing for one pane, for faster jumps in deep trees.
8. **Act on find-files / find-in-files results.** Multi-select hits in the
   Telescope-style popups and batch-delete/copy/move them through the
   existing `fs/ops.rs` worker, instead of only navigate/open-in-`$EDITOR`.
9. **Remote panel via SSH tooling.** Shell out to `ssh`/`sftp`/`rsync` (same
   philosophy as shelling to `git` rather than linking `libgit2`) to browse
   and transfer to a remote host. Highest effort of the list; only worth it
   if remote workflows matter to users.

*Archive VFS was #1 here — done, see Completed below.*

---

## Completed

### Feature ideas, done so far

- **Archive VFS.** `Enter` on a `.zip`/`.tar`/`.tar.gz` now switches the pane
  into a read-only virtual listing of its contents instead of opening it in
  `$EDITOR`; `Enter`/`Backspace` navigate in and out (stepping back out at
  the archive's root returns to the real directory that contains it, which
  is never rewritten while browsing), and `Copy` extracts the selection into
  the other pane through the same worker-thread transfer machinery as a
  normal copy — `fs::archive::spawn_extract` reports progress over the same
  `ProgressMsg` channel `fs::ops::spawn_transfer` does, so the progress
  gauge and its cancellation needed no new UI code. `Move`, and every write
  action (create/rename/delete/bulk-rename/paste/dir-size), is refused with
  a footer error while a pane is in archive mode. `fs::archive::list_entries`
  synthesizes any ancestor directories an archive did not store explicitly,
  so navigation always has a full breadcrumb even for a zip built file-by-
  file. It is a new, separate module rather than a reuse of
  `popup_preview.rs`'s existing `zip_listing`/`tar_listing`: those produce a
  flat "name  size" text block for a human to read, the VFS needs a real
  is-a-directory hierarchy to walk — sharing one function would have forced
  an awkward shape onto one side or the other. Nested archives and opening
  a file from inside one are explicitly out of scope.

### 0.2.0 — the seven priority items

- **1. `App::render` syncing state mid-draw.** The draw is now data-in,
  paint-out. `App::prepare_frame` settles everything the frame reads first:
  the preview popup's file-type probe and loader spawn, and both search
  popups' preview builds, all used to run inside the `terminal.draw` closure.
  Image previews were the worst case — the file was re-read and re-decoded on
  *every frame*, and `Picker::from_query_stdio` (which spawns `tmux` and
  blocks reading stdin) ran with it. Images decode once on a worker thread,
  the terminal is probed once behind a `OnceLock`, and the protocol is cached
  per area, failures included.
- **2. `App::run` doing six jobs in 108 lines.** Split into `draw_frame`,
  `absorb_filesystem_events`, `wait_for_input`, `run_pending_editor` and
  `run_pending_shell_command`; `run` itself is now 12 lines. Note the original
  note was wrong about terminal setup/teardown: `ratatui::run` in `main` owns
  those, so there was no `init`/`shutdown` pair to extract.
- **3. Unbounded `read_to_string` in the preview.** `build_preview` streams
  the file instead of slurping it, stops at the last line it needs, and caps
  the read at 8 MiB — so a huge log, or a binary with no newline in it, cannot
  be pulled into memory. Lines before the window are walked past and dropped,
  which bounds the line buffer as well as the byte count.
- **4. `Entry::new` syscall count.** One `lstat` now answers the kind, the
  permissions and the owner; the target is only `stat`ed when there is a link
  to follow. Three syscalls became one for an ordinary entry, six became three
  for a symlink. The listing reuses `DirEntry`'s own stat rather than throwing
  it away. (The note said `src/entry.rs`; the type lives in `ui/panes.rs`.)
- **5. `config.rs` ↔ `ui` circular dependency.** `SortType`, `SortOrder` and
  `ActivePane` moved to `src/types.rs`, which depends on nothing but serde.
  `config` no longer mentions `ui` at all. `ui/uiconfig.rs` is gone.
- **6. Git on the UI thread.** `git::PendingRepoInfo` runs `git status` on a
  worker thread; the listing draws immediately and the status column fills in
  when the answer arrives. (The note said `repo_info` already spawned threads
  and mentioned a `git log` call — neither was true: `git.rs` had no threads
  at all, and there is no `git log`.)
- **7. `Component::render`'s vestigial `_ui` parameter.** Already done before
  0.2.0, in `46b8a99`. The entry was stale.

### 0.1.0 and earlier

- B1: theme.rs `parse_hex` rewrite + `HexColor` newtype + fallback on bad theme
- B2: runtime HOME lookup + repair stale configs
- B3: `Chord::label()` replaces `pretty_key`
- B4: `SELECTION_HINTS` resolved through keymap + `resolve_hints` shared
- #1: Dead code removal
- #2: `Config::default` from empty TOML
- #3: `UiConfig` deleted from `Component::render`
- #4a: `Transfer` enum unifies copy/move
- #4b: `PreviewPane<K>` shared between finders
- #5: `palette!` macro
- #5b: `SYNTAX_RULES` moved to `ui/syntax.rs`
- #6a: `Highlighter` enum compiled once per frame
- #6b: `Pane` uses `entries` + `visible` index list
- #7: `handle_main_key` split + sort rotation on types
- #8a: single `GitStatus` + `repo_info` (2 spawns)
- #10: `Overlay`/`InputMode` enums, `UiConfig` deleted
- #23: destructive module tests (bulk rename, trash)
- Esc: no longer quits, pure dismiss chain
