# TODO

## Priority Items

### 1. `App::render` syncs state/filesystem mid-draw
`src/ui/mod.rs` — `render()` calls `sync_header()` and filesystem reads during the draw phase. State should be prepared before `ratatui::run` starts the frame, not inside it. Moving sync out of `render` would make the draw path purely data-in, paint-out.

### 2. `App::run` ~106 lines doing 6 jobs
`src/ui/mod.rs` — `run()` orchestrates terminal setup, event loop, rendering, and cleanup. Splitting into smaller functions (e.g. `init_terminal`, `event_loop`, `shutdown`) would improve readability and testability.

### 3. `filepreview.rs` sync `read_to_string` with no size cap
`src/ui/filepreview.rs` — `render_preview` does `fs::read_to_string` on the full file with no size guard. Large files will block the UI. Add a byte limit or switch to async/streaming reads.

### 4. `Entry::new` ~6 syscalls per entry
`src/entry.rs` — each `Entry::new` call does `metadata`, `symlink_metadata`, `read_link`, etc. Reducing syscall count (e.g. batch stat calls or caching) would speed up directory listings significantly.

### 5. `config.rs` ↔ `ui` circular dependency
`src/config.rs` defines `SortType` and `ActivePane` which live in `ui/` but are serde-persisted. This creates a circular dependency between config and UI layers. Extract shared types into a small `src/types.rs` or similar.

### 6. Git still synchronous on UI thread
`src/ui/git.rs` — `repo_info` spawns background threads, but initial git calls (`git status`, `git log`) on the UI thread can still block. Needs measurement first to confirm if this is actually a problem in practice.

### 7. `Component::render` still has `_ui` cleanup
`src/ui/component.rs` — the `UiConfig` struct was deleted but the `_ui` parameter remains in the trait signature (now `_ui: &()`). Clean up the trait to remove the vestigial parameter entirely.

---

## Completed (reference)

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
