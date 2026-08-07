# TODO

## Priority Items

Nothing outstanding. The seven items that stood here were closed for 0.2.0 —
see below for what each turned out to be.

---

## Completed

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
