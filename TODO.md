# rodeo — Development TODO

> **Last updated:** 2026-07-31
> **Current state:** Dual-pane navigation + rich preview + theming + git-colored entries + full file operations. **Phases 1–4 essentially complete.** Preview: syntax highlighting, images, archives (zip/tar/tgz), PDF, directory size, binary info+hexdump, symlink status — slow content is built on a worker thread behind a spinner, cached per selection, with page/half-page scrolling (Ctrl+f/b/d/u). Configurable keybindings (`[keybindings]` in config.toml), trash view (`:trash`), bulk rename (`B`), find-in-files (`Ctrl+g`), on-demand directory sizes (`S`), wildcard selection (`*`), live directory refresh via `notify`, transient footer status messages instead of modal error dialogs. Fuzzy search + regex filter with match highlighting. Config and themes are TOML, resolved through an XDG search path. 125 unit tests + 7 integration tests. Clippy-clean, fmt-clean, CI workflow added.

| Phase | README Milestone | Focus |
|-------|-----------------|-------|
| Phase 0 (Stability) | — | Pre-M2 bug fixes, crash fixes, dead code removal |
| Phase 1 (File Ops) | M2 | Copy, move, delete, rename, mkdir, dialogs, command palette |
| Phase 2 (Search) | M3 | Fuzzy search, regex filter, find-in-files |
| Phase 3 (Preview) | M4 | Syntax highlighting, archive preview, file watching |
| Phase 4 (Power User) | M5 | Bulk rename, trash, shell commands, directory sizes |
| Phase 5 (Infrastructure) | — | Tests, CI/CD, error handling, logging, docs |
| Phase 6 (UI Polish) | pre-release | Popup sizing, syntax colours, pane layout, chrome |

---

## Quick Wins *(< 30 min each)*

These are small, self-contained fixes with high impact-to-effort ratio. Do them first to build momentum.

- [x] **Fix footer lies (Also tracked as 0.6):** Replace "x Cut"/"p Paste" with actual bindings ("x Select", "Space Preview", "Tab Switch", "q Quit", "? About")
  - **P0** | **Files:** `src/ui/footer.rs:18-24` | **Hints:** Change the `keymaps` vec strings to match real bindings in `input.rs`. | **Effort:** S

- [x] **Remove `active_keybind_popup` dead code (Also tracked as 0.12):** Flag is never set to `true` — popup can never appear. Either wire it to a keybinding or delete the field + render call.
  - Resolved: wired to `F1`, popup populated with the real binding list.
  - **P1** | **Files:** `src/ui/uiconfig.rs:14`, `src/ui/mod.rs:87-89`, `src/ui/input.rs:33,40,149,153,165` | **Hints:** If keeping, bind `F1` or `K` to toggle it. If removing, delete the field, the render block in `mod.rs`, and all references in `input.rs`. | **Effort:** S

- [x] **Fix `FileOpened` empty match arm (Also tracked as 0.5):** Enter on a file returns `FileOpened(_entry)` but the match arm at `input.rs:126` is empty `{}`. Open the file with `$EDITOR`/`$VISUAL`.
  - Resolved: `FileOpened` sets `pending_editor_file`; the run loop spawns `config.editor` (defaults to `$EDITOR`, fallback `vi`) and reloads the pane on exit. `$VISUAL` support tracked in 4.6.
  - **P0** | **Files:** `src/ui/input.rs:126` | **Hints:** `std::process::Command::new(env::var("EDITOR").unwrap_or("vim".into())).arg(&entry.path).status()`. Need `use std::env;`. | **Effort:** S

- [x] **Fix `ReadOnly` config unused (Also tracked as 0.13):** `config.read_only` field parsed from YAML but never checked. Gate file operations (cut/copy/delete/rename) behind it.
  - **P1** | **Files:** `src/config.rs:46` (field), `src/ui/input.rs` (check before ops) | **Hints:** Add `pub fn is_read_only(&self) -> bool` to `Config`. Check in `handle_main_key` before allowing destructive operations (once implemented). | **Effort:** S
  - The ReadOnly option was removed. Will be maybe re-added if there's a need for it.

- [x] **Remove `color-eyre` from Cargo.toml or use it:** Dependency declared but never imported. Either integrate it (replace `env_logger` with `color-eyre` + `tracing`) or drop it to reduce compile times.
  - Resolved: integrated. `color_eyre::install()?` at top of `main()`, `main()` now returns `color_eyre::Result<()>` (also resolves 5.3.2).
  - **P1** | **Files:** `Cargo.toml:10` | **Hints:** If keeping, add `color_eyre::install()?;` at top of `main()`. If dropping, change `io::Result<()>` to use `eyre::Result` separately. See Phase 5 for full error handling plan. | **Effort:** S

- [x] **Populate About popup:** Currently renders an empty titled block. Add app name, version, author, and repository link.
  - Resolved: renders name, `CARGO_PKG_VERSION`, description, author, and Codeberg link, centered.
  - **P2** | **Files:** `src/ui/popup_about.rs` | **Hints:** Add a `Paragraph` inside the block with app info. Use `env!("CARGO_PKG_VERSION")` for version. Center-text align inside the block. | **Effort:** S

- [x] **Populate Keybinds popup:** Currently renders an empty titled block. List actual keybindings from `input.rs`.
  - **P2** | **Files:** `src/ui/popup_keybinds.rs` | **Hints:** Create a static `&[(&str, &str)]` mapping key to description. Render as a two-column table or list. Wire popup toggle to a keybinding (e.g., `F1` or `K`) if keeping `active_keybind_popup`. | **Effort:** S

- [x] **Fix header info placeholder:** Header hardcodes `"~info one"` in `App::new()`. Show active pane name or total file count.
  - Resolved: placeholder removed. Header now shows live `PaneStats` (selected/files/dirs/hidden) on the left and git status on the right.
  - **P2** | **Files:** `src/ui/mod.rs:49` | **Hints:** Replace with useful metric: total files in active directory, app version string, or remove the field entirely. The Header is a single shared widget (not per-pane), so pane-specific info belongs elsewhere. | **Effort:** S

- [x] **Fix Cargo.toml edition:** `edition = "2024"` has been stable since Rust 1.85. Confirm minimum supported Rust version and adjust edition accordingly.
  - Resolved: keeping `edition = "2024"`. Toolchain is rustc 1.95.0 stable; edition 2024 is stable since 1.85. MSRV is effectively 1.85.
  - **P1** | **Files:** `Cargo.toml:4` | **Hints:** Check `rustup show`. If stable, change to `"2021"`. | **Effort:** S

- [x] **Fix `println!` debug statement in `main.rs:27`:** `println!("Config: {:?}", config);` leaks to terminal. Replace with `info!` or remove.
  - Resolved: replaced with `log::debug!` (`main.rs:26`).
  - **P2** | **Files:** `src/main.rs:27` | **Hints:** `info!("Config: {:?}", config);` | **Effort:** S

- [x] **Add `.gitignore` entries:** Missing `*.swp`, `*.swo`, `.*.swp`, `*.log`, `/themes/*.swp`.
  - **P2** | **Files:** `.gitignore` | **Hints:** Standard Rust + editor ignore patterns. | **Effort:** S

- [x] **Bind `Ctrl+L` to manual redraw/refresh**
  - Resolved: `Ctrl+l` reloads both panes; documented in the F1 keybinds popup.
  - **P2** | **Files:** `src/ui/input.rs:67-77` | **Hints:** In `handle_ctrl_key`, add `KeyCode::Char('l') => { self.panes.reload(&self.config, false); return true; }`. This forces a re-read of both panes' directories and a full terminal redraw. Essential when terminal state gets corrupted. | **Effort:** S

---

## Phase 0: Bug Fixes & Code Health

> Goal: Stop crashing. Remove dead code. Make existing features work correctly.
> Dependencies: None. Everything here can start immediately.

- [x] **0.1 Fix crash: unknown Ctrl keys call `todo!()` → PANIC**
  - **P0** | **Files:** `src/ui/input.rs:75` | **Hints:** Replace `_ => todo!("no action defined while pressing CTRL")` with `_ => return false`. Unknown Ctrl combos should be a no-op, not a crash. Use `log::debug!("unhandled Ctrl+{:?}", key.code)`. | **Effort:** S

- [x] **0.2 Fix crash: unknown Shift keys call `unimplemented!()` → PANIC**
  - **P0** | **Files:** `src/ui/input.rs:112` | **Hints:** Replace `_ => unimplemented!("SHIFT key {} not yet implemented.", key.code)` with `_ => return false`. Log the unhandled key. | **Effort:** S

- [x] **0.3 Fix crash: preview of non-file items calls `todo!()` → PANIC**
  - **P0** | **Files:** `src/ui/input.rs:170` | **Hints:** Replace `_ => todo!("Preview of non-files not implemented yet")` with graceful handling. For `EntryKind::Parent`, show "Cannot preview parent directory". For `EntryKind::Unknown`, show "Unknown file type — cannot preview". Use `log::warn!`. | **Effort:** S

- [x] **0.4 Fix `handle_popup_key` always returns `true` (consumes all keys when popup active)**
  - **P0** | **Files:** `src/ui/input.rs:38-65` | **Hints:** The fallthrough `true` at line 64 means ANY key when a popup is active gets consumed — including `Esc` after it was already handled. Change the final `true` to `false` (return false = didn't handle, let main handler try). Test with `Esc` in preview popup: it should close preview, not quit. | **Effort:** S

- [x] **0.5 Fix Enter on file does nothing (empty `FileOpened` arm)**
  - **P0** | **Files:** `src/ui/input.rs:126` | **Hints:** (See also Quick Wins above — this is so critical it appears in both lists.) Spawn `$EDITOR` with the file path. On editor exit, reload pane to reflect any changes. Use `std::env::var("EDITOR").unwrap_or_else(|_| "vi".into())`. | **Effort:** S

- [x] **0.6 Fix footer: replace misleading keybindings with true ones**
  - Footer shows: F1 Keys, Space Preview, F4 Edit, Tab Panes, x Select, ^h Hidden, ? About, F10 Quit. mc-style F-row roadmap: F2 Rename (1.2.4), F3 Search (2.2), F5 Copy (1.2.1), F6 Move (1.2.2), F7 Mkdir (1.2.5), F8 Delete (1.2.3) — each joins the footer only once implemented.
  - **P0** | **Files:** `src/ui/footer.rs:18-24` | **Hints:** (See also Quick Wins above.) Current lies: `x Cut` (x=toggle select), `p Paste` (p=unbound), `SPACE select` (SPACE=preview), `? preview` (?=about). Replace with `h/j/k/l Move`, `Enter Open`, `Tab Switch`, `x Select`, `Space Preview`, `q Quit`, `? About`. Make Ctrl+h and Backspace visible too. | **Effort:** S

- [x] **0.7 Fix preview: `self.selected.as_ref().unwrap()` panics if selected is None at render time**
  - **P1** | **Files:** `src/ui/popup_preview.rs:119` | **Hints:** If the entry was deleted between opening preview and rendering, the `unwrap()` crashes. Change to `let Some(entry) = self.selected.as_ref() else { return; };`. | **Effort:** S

- [x] **0.8 Document popup key precedence chain in `handle_popup_key`**
  - **P1** | **Files:** `src/ui/input.rs:38-77` | **Hints:** The current precedence order (popup keys → ctrl keys → shift keys → main keys) is correct. Ctrl+Up/Down in preview work because they return `true` before `handle_ctrl_key` runs. Add a comment at the top of `handle_popup_key` documenting the chain: "Keys checked in order: popup-specific → Ctrl-modified → Shift-modified → unmodified. Popup handler takes priority when any popup is active." | **Effort:** S

- [x] **0.9 Make popups dismissible with `q` and `Esc` consistently**
  - **P1** | **Files:** `src/ui/input.rs:38-65,139-160` | **Hints:** In `handle_popup_key`, add `KeyCode::Char('q')` alongside `Esc` to close all popups. Mirror `Esc` behavior: if popups active, close them; otherwise quit. | **Effort:** S

- [x] **0.10 Add `g`/`G`/`gg` Vim-style jump navigation**
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** `gg` = jump to top of file list, `G` = jump to bottom, `g` = prefix for other motions (future). Add `Pane::goto_first()` and `Pane::goto_last()` methods that set `self.state.select(Some(0))` and `self.state.select(Some(max))` respectively. In `handle_main_key`, match `KeyCode::Char('g')` — since `g` is a prefix, you'll need a small state machine: first `g` sets a flag, second `g` triggers `goto_first()`. Same for `G` (single key). | **Effort:** S

- [x] **0.11 Audit and tag all `.unwrap()` / `.expect()` calls for systematic replacement** — all `.unwrap()` removed. 2 intentional `.expect()` remain: `config.rs:128` (XDG path, unrecoverable), `theme.rs:404` (generated theme parse, unrecoverable).
  - **P1** | **Files:** `src/config.rs:105, 114, 123, 128, 131, 133, 134, 136, 140`, `src/ui/panes.rs:139,263,291,393`, `src/ui/popup_preview.rs:89,92,94,147,153-154`, `src/ui/theme.rs:133,136,163` | **Hints:** Count: ~22 unwrap/expect sites. Tag each with `// TODO(#error-handling):`. Replace with `?` operator after converting functions to return `Result`. Most panics happen on: filesystem errors (permissions, missing files), `file` command failures, image decode failures. | **Effort:** M

- [x] **0.12 Remove dead code: `active_keybind_popup` never activated** — wired to `F1`.
  - **P1** | **Files:** See Quick Wins above. | **Hints:** (See also Quick Wins above.) Note: `active_keybind_popup` is only ever set to `false` in the codebase. The `?` key sets it to `false` (line 149), and Esc sets it to `false` (line 40). It is never set to `true` — so the popup can never appear. Either bind to key (recommend `F1` or `K`) with populated content, or remove: `uiconfig.rs:14`, `mod.rs:87-89`, `input.rs` references at lines 33,40,149,153,165. | **Effort:** S

- [x] **0.13 Remove dead code: `Config::read_only` never checked**
  - Resolved: the `read_only` field was removed from `Config` entirely. May be re-added if a need arises.
  - **P1** | **Files:** `src/config.rs:46`, `src/ui/input.rs` | **Hints:** (See also Quick Wins above.) Add getter. Check in `handle_main_key` before allowing: cut/copy/delete/rename (once implemented), toggle_select, Enter-on-file. Show a status message "Read-only mode — operation blocked" in footer. | **Effort:** S

- [x] **0.14 Fix config format mismatch: code uses YAML, README/planning docs say TOML**
  - Resolved (2026-07-31): switched to **TOML**. `yaml_serde` → `toml`, `config.yaml` → `config.toml`, all ten bundled themes converted (`[colors]` table). A leftover `config.yaml` is reported on stderr instead of being silently ignored. Fixed two bugs it exposed: the default theme name (`light`) had no theme file, and a missing theme file called `process::exit(1)` instead of falling back to the default.
  - **P2** | **Files:** `src/config.rs`, `src/ui/theme.rs`, `Cargo.toml`, `themes/*.toml` | **Effort:** M

- [x] **0.15 Fix `Entry::parent()`: `canonicalize().unwrap()` panics if parent doesn't exist or permissions deny**
  - **P1** | **Files:** `src/ui/panes.rs:139` | **Hints:** `PathBuf::from(dir).join("..")` then `canonicalize()` — if at filesystem root, `..` is still root (no panic). But permission errors or deleted directories will crash. Use `.ok()?` fallback: return an Entry with `kind: Parent` and the joined path (canonicalization is a nice-to-have). | **Effort:** S

- [x] **0.16 Fix `read_entries()`: `fs::read_dir(dir).unwrap()` panics on permission denied or missing dir**
  - **P1** | **Files:** `src/ui/panes.rs:393` | **Hints:** Use `match fs::read_dir(dir) { Ok(rd) => rd, Err(e) => { log::error!("Cannot read {}: {}", dir, e); return vec![Entry::parent(dir)]; }}`. | **Effort:** S

- [x] **0.17 Fix `Pane::open()` canonicalize unwrap can panic**
  - **P1** | **Files:** `src/ui/panes.rs:291-292` | **Hints:** `entry.path.canonicalize().unwrap()` — permissions or broken symlinks crash. Use `match entry.path.canonicalize() { Ok(p) => ..., Err(e) => { log::error!(...); return OpenAction::Nothing; }}`. | **Effort:** S

- [x] **0.18 Check `handle_main_key` `Esc` logic: quits app if no popups active**
  - **P1** | **Files:** `src/ui/input.rs:152-160` | **Hints:** This means `Esc` without any popup quits the app. MC tradition uses `F10` or `q` for quit. This is a design choice — confirm it's intentional. Recommendation: make Esc a no-op when no popups are open, to prevent accidental exits. Only `q` (and `:q`) should quit. | **Effort:** S

- [x] **0.19 Prevent preview of `..` (parent entry)**
  - **P2** | **Files:** `src/ui/input.rs:161-173` | **Hints:** Currently `Space` on `..` tries to preview — `EntryKind::Parent` falls into the `todo!()` crash in 0.3. After fixing 0.3, still show a "Cannot preview parent directory" message rather than an error. | **Effort:** S

- [x] **0.20 Add runtime dependency documentation**
  - bat replaced with syntect (pure Rust) — done.
  - file command replaced with infer crate (pure Rust) — done.
  - No runtime dependencies remain for preview.
  - **P1** | **Files:** `README.md` | **Hints:** README still needs updating to reflect the removal of bat/file runtime deps. Update README to note preview works without external binaries. | **Effort:** S

- [x] **0.21 Sanitize header directory display for very long paths**
  - **P2** | **Files:** `src/ui/panes.rs:383` | **Hints:** `self.path.to_string()` is used as pane title. Very long paths overflow the pane border. Truncate with `...` prefix: `format!("...{}", &path[path.len().saturating_sub(width)..])`. | **Effort:** S

---

## Phase 1: File Operations (M2)

> Goal: Copy, move, delete, rename, mkdir — the core of a file manager.
> Depends on: Phase 0 completed (no crashes, unwrap cleanup).
> Key design: Vim-style modal keybindings (y=yank, d=cut, p=paste, dd=delete, r=rename).

### 1.1 Confirmation Dialog Infrastructure

- [x] **1.1.1 Create `src/ui/dialog.rs` — generic confirmation popup module**
  - Resolved: `Dialog` with `DialogKind` + `DialogAction` (what to do on confirm) + `DialogResult`. Stored as `App.dialog: Option<Dialog>` (like `preview`) instead of `UiConfig` — avoids borrow conflicts.
  - **P0** | **Files:** `src/ui/dialog.rs` (new), `src/ui/mod.rs` | **Hints:** A modal dialog that displays a message (e.g., "Delete 3 files?") with Yes/No options. Take a title string, message string, and two callbacks or return a `ConfirmResult` enum. Render as centered block with borders, keyboard navigation (y/n or Enter/Esc). Add `active_dialog: Option<Dialog>` to `UiConfig`. | **Effort:** M

- [x] **1.1.2 Implement `Dialog` with actions: confirm, confirm_multi, input (for rename/mkdir)**
  - Resolved: three variants implemented — `Confirm { message }`, `Input { prompt, value }` (with cursor), `Message { text }`. All in live use: Input for mkdir/touch, Confirm for touch-overwrite, Message for op errors.
  - **P0** | **Files:** `src/ui/dialog.rs` | **Hints:** Three variants: `Confirm { title, message }`, `Input { title, prompt, value }`, `Message { title, text }`. Use an enum `DialogKind`. Render input dialogs with a cursor and single-line text field. | **Effort:** M

- [x] **1.1.3 Add dialog input handling in `App::handle_input`**
  - Resolved: dialog keys routed before all other handlers; Enter=confirm/submit, Esc=cancel, Backspace=delete char, text=append (plain + Shift).
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/mod.rs` | **Hints:** If dialog is active, route keys to dialog handler (Enter=confirm, Esc=cancel, for input dialogs: backspace=delete, text=append). Dialogs take priority over all other key handling. | **Effort:** M

- [x] **1.1.4 Add dialog rendering in `App::render`**
  - Resolved: rendered last (on top of popups), centered clear+block+paragraph.
  - **P0** | **Files:** `src/ui/mod.rs:70-104` | **Hints:** After popups, render dialog as a centered clear+block+paragraph. Dialogs should render on top of everything. | **Effort:** M

### 1.2 File Operations — Synchronous (Phase 1a)

- [x] **1.2.1 Implement file copy (single file)**
  - Resolved: `F5` copies the highlighted entry to the inactive pane's directory. Directories copied recursively (`copy_dir_recursive`). Overwrite Confirm dialog on name clash; guards against same-file copy and copy-into-own-subdirectory. Vim yank/paste (`y`/`p`) arrives with modal bindings in 1.3.3.
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** Vim binding: `y` yanks (copies path to clipboard buffer), `p` pastes. Non-modal for now: `F5` or `Ctrl+C` copies. Implementation: `std::fs::copy(src, dst)`. Show confirmation if destination exists. Copy the `Entry` from active pane to inactive pane's directory. | **Effort:** M

- [x] **1.2.2 Implement file move (single file)**
  - Resolved: `F6` moves via `std::fs::rename`; on failure (e.g., cross-device EXDEV) falls back to copy + delete. Overwrite Confirm dialog on name clash.
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** Vim binding: `d` cuts (moves path to clipboard buffer), `p` pastes. Non-modal: `F6`. Implementation: `std::fs::rename(src, dst)`. Falls back to copy+delete for cross-device moves. Show confirmation. | **Effort:** M

- [x] **1.2.3 Implement file delete (single file, to trash)**
  - Resolved: `F8`/`Delete` → "Move to trash?" Confirm → `trash::delete`. On trash failure, second Confirm offers permanent delete (per Open Decision 9). `dd` binding deferred to 1.3.4 (needs key-sequence state). Added `F8 Delete` to footer.
  - **P0** | **Files:** `src/ui/input.rs`, `Cargo.toml` | **Hints:** Use `trash` crate. Add `trash = "5"` to Cargo.toml. Binding: `dd` (or `Delete` key, or `F8` mc-style). Show confirmation dialog: "Move 'filename' to trash?". If trash fails (e.g., on some filesystems), offer permanent delete as fallback with extra confirmation. Add `F8 Delete` to the footer when implemented. | **Effort:** M

- [x] **1.2.4 Implement file rename (inline or dialog)**
  - Resolved: `r` and `F2` open an Input dialog pre-filled with the current name. Name-clash → overwrite Confirm; same name → no-op. Errors (permissions, invalid names) shown in a Message dialog. Added `F2 Rename` to footer.
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/dialog.rs` | **Hints:** Binding: `r` and `F2` (mc-style) on a file. Open an input dialog pre-filled with the current filename. On confirm, `std::fs::rename(old, new)`. Reload pane afterward. Handle errors: name collision, invalid chars, permissions. Add `F2 Rename` to the footer when implemented. | **Effort:** M

- [x] **1.2.5 Implement mkdir**
  - Resolved: bound to `F7`. Input dialog → `std::fs::create_dir()`, pane reload on success, Message dialog on error. Empty name rejected. Added to footer + keybinds popup.
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/dialog.rs` | **Hints:** Binding: `F7` or `Ctrl+N`. Open input dialog with prompt "Directory name:". Create with `std::fs::create_dir()`. Reload pane. Handle errors: already exists, permission denied. | **Effort:** S

- [x] **1.2.6 Implement touch (create empty file)**
  - Resolved: bound to `Ctrl+T`. Input dialog → `std::fs::File::create()`; if file exists, Confirm dialog asks before truncating. Note: `Ctrl+T` now taken — theme switching (3.1.4) must use another binding or `:theme`.
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/dialog.rs` | **Hints:** Binding: `Ctrl+T` or similar. Input dialog for filename. Create with `std::fs::File::create()`. Reload pane. | **Effort:** S

- [x] **1.2.7 Add selection-aware batch operations**
  - Resolved: copy/move/delete operate on all `x`-selected entries (falling back to the highlighted entry). Directories and symlinks are now selectable too. Batch delete asks "Move N items to trash?"; copy/move count name clashes in one overwrite confirm. `Esc` clears the selection (before quitting). Selection marks sync between the visible and full lists and survive reloads.
  - **P1** | **Files:** `src/ui/panes.rs`, `src/ui/input.rs` | **Hints:** When multiple files are selected (via `x`), operations apply to all selected files. For copy/move, the target is the other pane's directory. For delete, show "Delete N files?". Gather selected entries: `pane.paths.iter().filter(|e| e.selected).collect()`. | **Effort:** M

- [x] **1.2.8 Update footer to show contextual keybindings**
  - Resolved: when items are selected, the footer switches to `●N selected`, `F5 Copy`, `F6 Move`, `F8 Delete`, `Esc Unselect`; otherwise the full key list is shown. (Vim-mode bindings land with 1.3.)
  - **P1** | **Files:** `src/ui/footer.rs` | **Hints:** After implementing operations, show actual bindings: `y Yank`, `d Cut`, `p Paste`, `r Rename`, `dd Delete`, `Ctrl+n Mkdir`, `Ctrl+t Touch`, `x Select`. Make footer dynamic: when files are selected, show selection count and bulk-action hint. | **Effort:** S

### 1.3 Vim-Style Modal Keybindings (Phase 1b)

- [x] **1.3.1 Add mode state machine: Normal, Command, Visual**
  - Resolved: implemented with a `Mode` enum + footer indicator, then **simplified away** (2026-07-26): after visual mode was dropped, `Command` state was already tracked by `App.command`, so the enum was redundant and was deleted. No mode tracking remains — none is needed.
  - **P1** | **Files:** `src/ui/uiconfig.rs` or new `src/ui/mode.rs` | **Hints:** `enum Mode { Normal, Command, Visual, Insert }`. `Normal` is default (navigation). `Command` is `:` palette. `Visual` is selection mode (entered via `v`). Track in `UiConfig` or `App`. Render mode indicator in footer: `-- NORMAL --`, `-- VISUAL --`, `-- COMMAND --`. | **Effort:** M

- [x] **1.3.3 Implement yank (`y`) and put (`p`) clipboard**
  - Resolved: `y` yanks selected-or-current into `App.clipboard: Vec<PathBuf>`; `p` pastes a copy into the *active* pane dir; `P` pastes as move and clears the clipboard. Overwrite → confirm dialog (`PasteMove` action). Footer shows `[N yanked]` / `[N cut]`. Cut state is armed via `P` at paste time, so no separate cut command needed.
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** Internal clipboard: `Vec<PathBuf>` in `App` or `UiConfig`. `y` yanks selected (or current) file path to clipboard. `p` pastes (copies) from clipboard to current pane directory. `P` for move (cut+paste). Show clipboard count in footer: `"[2 files yanked]"`. | **Effort:** M

- [x] **1.3.4 Implement `dd` (delete current file or all selected)**
  - Resolved: pending-key state (`pending_d`); any other key cancels the chord. `dd` → trash-confirm on selected-or-current.
  - **P1** | **Files:** `src/ui/input.rs` | **Hints:** `dd` = delete the selected-or-highlighted entries (with confirmation). | **Effort:** M

- [x] **1.3.5 Implement `r` (rename current file)**
  - Resolved: `r` already opened the rename dialog (1.2.4). Bulk rename of selections is Phase 4 task 4.1.
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/dialog.rs` | **Hints:** `r` opens the rename dialog for the currently highlighted file; bulk rename of a multi-selection is 4.1. | **Effort:** M

### 1.4 Command Palette

- [x] **1.4.1 Create command palette popup (`:` key)**
  - Resolved: `:` opens a command bar above the footer (reuses the input-bar layout + `TextInput` with cursor movement). Commands: `:q`/`:quit`, `:w`/`:write`, `:e`/`:cd <path>`, `:mkdir`, `:touch`, `:delete`, `:rename <new>`, `:theme [name]`, `:help`, `:shell`, `:!cmd`. Tab completes command names (common prefix) and theme names.
  - **P1** | **Files:** `src/ui/popup_cmd.rs` (new), `src/ui/mod.rs` | **Hints:** Press `:` to open a command input bar at bottom of screen (or centered popup). Model after Vim's command line. Commands: `:q` quit, `:w` save config, `:e <path>` navigate, `:mkdir <name>`, `:touch <name>`, `:delete`, `:rename <new>`, `:cd <path>`, `:theme <name>`, `:help`. Auto-complete commands with tab. | **Effort:** M

- [x] **1.4.2 Implement command parser and dispatcher**
  - Resolved: `run_command` splits command/rest (multi-word args preserved for rename/mkdir), dispatches by name, shows "Unknown command" message dialog for unrecognized input. `:!` is intercepted before tokenizing so shell args keep their spacing.
  - **P1** | **Files:** `src/ui/popup_cmd.rs` | **Hints:** Parse `:command [args...]`. Use a match on the command name. Show "Unknown command: foo" for unrecognized input. Reference: `xplr` and `lf` command syntax. | **Effort:** M

- [x] **1.4.3 Add `:!<shell command>` runner**
  - Resolved: `:!cmd` runs `sh -c`, captures stdout+stderr, shows up to 30 lines in a Message dialog (truncation marker beyond that). `:shell` spawns an interactive `$SHELL` subshell (terminal suspend/resume like the editor flow) and reloads panes on exit.
  - **P2** | **Files:** `src/ui/popup_cmd.rs` | **Hints:** `:!ls -la` runs a shell command and shows output in a scrollable popup or preview pane. Use `std::process::Command::new("sh").args(["-c", cmd])`. Suspend UI during execution, capture stdout/stderr, display result. Add `:shell` to spawn an interactive subshell. | **Effort:** M

### 1.5 Async Operations + Progress (worker thread + mpsc channel, no tokio)

- [x] **1.5.2 Implement async file copy with progress bar**
  - Resolved: transfers >10MB run in the background (`ops::spawn_transfer`): chunked 256KB copy reports bytes via channel, a `Gauge` dialog shows percent + "Esc to cancel". Esc sets the cancel flag; the worker removes the partial file and aborts. UI stays fully responsive during transfer.
  - **P1** | **Files:** `src/fs/ops.rs` (new), `src/ui/dialog.rs` | **Hints:** When copying large files (>10MB) or multiple files, spawn a dialog with a progress bar. Stream chunks manually for progress tracking. Cancel via `Esc`. Progress = bytes_copied / total_bytes. | **Effort:** L

- [x] **1.5.3 Implement async file move with progress**
  - Resolved: same threshold path — `spawn_transfer(cut=true)` copies with progress, then deletes sources; clipboard clears on completion for cut-pastes. Small moves still use instant `rename`.
  - **P1** | **Files:** `src/fs/ops.rs` | **Hints:** Try `rename` first (instant). If `EXDEV` (cross-device), fall back to copy+delete with progress bar. | **Effort:** M

- [x] **1.5.4 Create `src/fs/mod.rs` and `src/fs/ops.rs` — extract file operation logic**
  - Resolved: `ops::copy_entry`, `move_entry` (rename + copy/delete fallback), `delete_entry`, `copy_dir_recursive`, `total_size`, `check_transfer_paths`, `file_name_of` + the async transfer workers. UI code calls these; tests live alongside.
  - **P1** | **Files:** `src/fs/mod.rs` (new), `src/fs/ops.rs` (new) | **Hints:** Move file operation implementations out of UI code. Functions: `copy_file(src, dst) -> Result`, `move_file(src, dst) -> Result`, `delete_file(path) -> Result`, `copy_dir(src, dst) -> Result` (recursive). Keep UI code thin. | **Effort:** M

---

## Phase 2: Search & Filter (M3)

> Goal: Find files fast. Fuzzy search, regex filter, search file contents.
> Depends on: Phase 0. Phase 1 is not strictly required but helps (command palette infrastructure).

- [x] **2.1 Add `nucleo` for fuzzy matching**
  - Resolved: `nucleo = "0.5"` (via `nucleo::pattern::Pattern` + `Matcher`) and `regex = "1"` added.
  - **P0** | **Files:** `Cargo.toml` | **Hints:** `nucleo = "0.5"`. Lightweight, no large deps. Alternative: `skim` (fzf-compatible but heavier). Planning docs recommend `nucleo`. | **Effort:** S

- [x] **2.2 Implement fuzzy file finder (`/` and `F3` keys)**
  - Resolved: `/` or `F3` opens a search bar above the footer. Live fuzzy-filter with `nucleo` (smart case), results ranked best-first, `..` pinned on top, arrows navigate, `Enter` jumps cursor to the top match and drops the filter, `Esc` cancels. Match-character highlighting is 2.6. `F3 Search` added to footer.
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/search.rs` (new), `src/ui/mod.rs` | **Hints:** Press `/` or `F3` (mc-style, decided in 0.6) to open a search bar at bottom of the active pane (or as a popup). Type to fuzzy-filter entries in the current directory. Results update in real-time as you type. `Esc` to cancel, `Enter` to select top match. Use `nucleo::Matcher` with the file names. The pane should highlight matching characters in filenames. Add `F3 Search` to the footer when implemented. | **Effort:** M

- [x] **2.3 Implement regex filter (`Ctrl+F`)**
  - Resolved: `Ctrl+F` opens the filter bar (pre-filled with the active pattern for editing). Live-filtered as you type; invalid regex shows the bar in the theme error color and keeps the last valid listing. `Enter` keeps the filter (bar stays visible read-only with pattern + hints); `Esc` in the bar cancels, `Esc` in main mode clears the filter before quitting. Filter re-applies automatically on pane reload. `regex = "1.13"` added.
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/search.rs` | **Hints:** Press `Ctrl+F` to open a regex input bar. Type a regex pattern to filter directory listing. Only files matching the regex are shown. Invalid regex shows error in the bar. Use the `regex` crate: `regex = "1"`. Add to `Cargo.toml`. | **Effort:** M

- [x] **2.4 Implement find-in-files (search file contents)**
  - Resolved: `Ctrl+G` opens a find-in-files popup with input bar. Enter regex pattern, press Enter to search current directory tree (via `ignore` crate, respects .gitignore). Results shown as `path:line: content` list. Navigate with arrows, Enter opens file in editor. Limited to 1000 matches. Binary files skipped automatically.
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/popup_findinfiles.rs` (new) | **Hints:** Press `Ctrl+G` or similar to open "grep" mode. Enter search term. Walk directory tree with `ignore` crate (respects .gitignore). Search each file with `ripgrep`-style line matching. Show results in a new pane or popup: `filename:line: match`. Allow `Enter` on a result to open the file at that line. Add `ignore = "0.4"` to `Cargo.toml`. | **Effort:** L

- [x] **2.6 Highlight search matches in file listing**
  - Resolved: fuzzy search (nucleo) highlights individual matching characters with `theme.colors.warning()` background; regex filter highlights the entire matched substring. Highlighting respects the entry's base style (git colors, symlink errors, etc.).
  - **P1** | **Files:** `src/ui/panes.rs`, `src/ui/search.rs` | **Hints:** When filter is active, render matching filename characters with a highlight color (e.g., yellow background or bold). Use ratatui `Span` with style for matched portion of filename. | **Effort:** M

---

## Phase 3: Preview & Polish (M4)

> Goal: Rich previews, remove external dependencies, live refresh.
> Depends on: Phase 0. Phase 2 (search) is recommended for find-in-files preview integration.

### 3.1 Syntax Highlighting — Replace `bat` with `syntect`

- [x] **3.1.1 Add `syntect` to dependencies**
  - **P0** | **Files:** `Cargo.toml` | **Hints:** `syntect = { version = "5", default-features = false, features = ["default-fancy"] }`. Sublime Text grammars bundled at compile time. No external binary needed. | **Effort:** S

- [x] **3.1.2 Implement syntax-highlighted text preview with syntect**
  - **P0** | **Files:** `src/ui/popup_preview.rs` | **Hints:** Use `syntect::easy::HighlightLines` which outputs `Vec<(Style, &str)>` directly — more efficient than HTML round-trip. Create a `SyntaxSet` and `ThemeSet` once (lazy_static or load at startup). For each file, detect language via extension using `SyntaxSet::find_syntax_by_extension`, then iterate `HighlightLines` to produce styled spans for ratatui. Fall back to plain text. This removes the `bat` runtime dependency entirely. | **Effort:** M

- [x] **3.1.3 Add theme-aware syntax highlighting**
  - **P1** | **Files:** `src/ui/popup_preview.rs`, `src/ui/theme.rs` | **Hints:** Map syntect's theme colors to rodeo's current theme. Or bundle a rodeo-specific Sublime Text theme. Use background/fg colors that work with the active rodeo theme. | **Effort:** M

- [x] **3.1.4 Implement runtime theme switching**
  - Resolved via command palette (1.4): `:theme <name>` switches at runtime (`self.theme = Theme::load_theme(...)`), `:theme` alone lists available themes, Tab completes theme names. Guards against the `load_from_file` process-exit on unknown names by validating against `get_theme_list()` first.
  - **P2** | **Files:** `src/ui/input.rs`, `src/ui/theme.rs`, `src/ui/mod.rs` | **Hints:** Add keybinding — NOTE: `Ctrl+T` is taken by touch (1.2.6), use `:theme <name>` via command palette or another key. At minimum, use `get_theme_list()` to discover available themes, cycle through them on keypress. Reload the `Theme` struct and trigger a full redraw. Since `App` owns `Theme`, replacement is straightforward: `self.theme = Theme::load(theme_name)?`. | **Effort:** S

- [x] **3.1.5 Resolve the themes directory at runtime instead of a relative path**
  - Found 2026-07-31 during the TOML migration: `DEFAULT_THEME_DIR` was the literal `"themes"`, so an installed binary only found themes when started from the source checkout.
  - Resolved: `theme_dirs()` searches `$XDG_DATA_HOME/rodeo/themes` → `$XDG_DATA_DIRS/rodeo/themes` → `./themes`, first match wins. `get_theme_list()` merges all of them and de-duplicates by name, so `:theme` completion works everywhere. The default theme is also compiled in with `include_str!`, and `load_from_file()` no longer calls `process::exit(1)` — loading degrades requested → default → built-in. Verified in a pty for four cases: no themes installed, themes in `$XDG_DATA_HOME`, source checkout, and user themes shadowing the checkout.
  - Remaining (not needed yet): a `theme_dir` config key to override the search entirely.
  - **P1** | **Files:** `src/ui/theme.rs`, `src/ui/input.rs` | **Effort:** M

### 3.2 Archive Preview

- [x] **3.2.1 Add `tar` and `zip` crates**
  - Resolved: `tar 0.4.46`, `zip 8.6` (no-default-features + deflate), `flate2 1.1.9`.
  - **P1** | **Files:** `Cargo.toml` | **Hints:** `tar = "0.4"`, `zip = "2"`. For `.tar.gz` use `flate2 = "1"`. | **Effort:** S

- [x] **3.2.2 Implement archive contents preview in preview pane**
  - Resolved: `.zip`/`.tar`/`.tar.gz`/`.tgz` list name + size in the preview (detected by extension with `infer` fallback), capped at 1000 entries. Preview content is now computed once per selection and cached (was re-read every frame).
  - **P1** | **Files:** `src/ui/popup_preview.rs` | **Hints:** When file type is Archive (detected by `file` command or extension), list archive contents instead of raw bytes. For `.zip`: iterate entries, show name + size + compressed size. For `.tar`/`.tar.gz`: iterate entries, show name + size + mode. Format as a table similar to the file listing. Limit to first 1000 entries to avoid hanging. | **Effort:** M

### 3.3 PDF/Document Preview

- [x] **3.3.1 Add PDF text extraction**
  - Resolved: `pdf-extract 0.12` — `.pdf` files (by extension) extract to scrollable text in the preview.
  - **P2** | **Files:** `Cargo.toml`, `src/ui/popup_preview.rs` | **Hints:** Use `pdf_extract = "0.7"` or `lopdf` for PDF text extraction. For `.docx`, use `docx-rs`. Extract text content and display in preview pane. Fall back to "Binary file — cannot preview" if extraction fails. | **Effort:** M

- [x] **3.3.2 Improve unknown file type handling**
  - Resolved: binary files show size, modification time, unix permissions (octal), MIME type (via `infer`), and a hex dump of the first 256 bytes. Symlinks show target path + broken-link detection.
  - **P1** | **Files:** `src/ui/popup_preview.rs:98-101` | **Hints:** Show more useful info for unknown/binary files: file size, MIME type, hex dump (first 256 bytes), file permissions, owner, modification time. Use `file` command output when available. | **Effort:** S

- [x] **3.3.3 Directory preview with total size**
  - Resolved (user request): directories preview to total recursive size, file/dir counts, and a children listing (dirs first, capped at 1000). Sizing uses `ops::total_size_capped` (50k-entry cap, shows "≥ X (partial)"); the walk never follows symlinks, so symlink cycles cannot hang it. Parent (`..`) and unknown-kind entries show a message instead of a filesystem preview.
  - **P2** | **Files:** `src/ui/popup_preview.rs` | **Effort:** S

### 3.4 File Watching

- [x] **3.4.1 Add `notify` crate for live directory refresh**
  - Resolved: `notify = "8"` added; `RecommendedWatcher` created at startup with an `mpsc` channel.
  - **P1** | **Files:** `Cargo.toml` | **Hints:** `notify = { version = "7", features = ["macos_kqueue"] }`. Cross-platform filesystem events. | **Effort:** S

- [x] **3.4.2 Implement auto-refresh on external changes**
  - Resolved: Both pane directories are watched (`NonRecursive`). The run loop drains events via `try_recv` (ignoring access/read events, which rodeo's own preview triggers), arms a 150 ms debounce `Instant`, and reloads panes on silence — keeping both the cursor position and the flagged entries. Watches re-sync automatically on navigation via `refresh_fs_watches()`. The 50 ms tick loop is extended to also run while `fs_debounce` is set so the reload fires promptly.
  - **P1** | **Files:** `src/ui/mod.rs`, `src/ui/panes.rs` | **Effort:** M

---

## Phase 4: Power User (M5)

> Goal: Bulk rename, trash, shell integration, directory sizes.
> Depends on: Phase 1 (operations).

- [x] **4.1 Bulk rename with regex and sequential patterns**
  - Resolved: `B` (or `Shift+B`) opens `BulkRename` popup on 2+ selected files. Pattern bar supports `s/regex/replacement/[g]` substitution and `prefix_%03d` sequential numbering. Live two-column old→new preview; collision and empty-name errors shown inline. Enter applies, Esc cancels.
  - **P1** | **Files:** `src/ui/popup_bulkrename.rs` (new), `src/ui/input.rs` | **Effort:** L

- [x] **4.2 Trash support with restore capability**
  - Resolved: `:trash` opens `TrashView` popup listing trashed items with original paths. `r` restores selected/highlighted items to their original location; `D` permanently deletes them; `x` multi-selects. Uses `trash::os_limited::{list, restore_all, purge_all}` (Linux/Windows). macOS shows a graceful "not supported" message.
  - **P1** | **Files:** `src/ui/popup_trash.rs` (new), `src/ui/input.rs` | **Effort:** L

- [x] **4.4 Directory size calculation**
  - Resolved: `Shift+S` → `Action::DirSizes` → `compute_dir_sizes()` walks each directory (capped at 200k entries), fills `entry.dir_size`, shows `≥X` when truncated. Size column shows the result in place of "DIR".
  - **P2** | **Files:** `src/fs/size.rs` (new), `src/ui/panes.rs` | **Hints:** For directories, show cumulative size instead of "DIR". Compute with parallel walk using `ignore` crate. Cache results. Show in the Size column: `12.3 MB` for dirs instead of `DIR`. Add a "calculating..." placeholder while scanning. `Ctrl+Shift+S` to trigger manual scan of current directory. | **Effort:** M

- [x] **4.5 Shell command output in preview pane**
  - Resolved: `:!cmd` already routes output to a `PopupPreview::from_text` scrollable popup. `%f` substitution (pipe selected files) added — `:!wc -l %f` expands `%f` to space-separated quoted paths of selected (or highlighted) entries.
  - **P2** | **Files:** `src/ui/popup_cmd.rs`, `src/ui/popup_preview.rs` | **Hints:** `:!<cmd>` captures stdout/stderr and shows it in the preview pane (reusing preview infrastructure). Allow piping selected files to commands: `:!wc -l %f`. | **Effort:** M

- [x] **4.6 External editor integration polish**
  - Resolved: `default_editor()` in `config.rs` already checks `$VISUAL` before `$EDITOR`, falls back to `vi`. Editor spawned in `App::run`; mtime compared before/after — footer shows "Modified: path" if changed. Directory reloaded on exit.
  - **P2** | **Files:** `src/ui/input.rs` | **Hints:** Support `$VISUAL` before `$EDITOR`. Add configuration option for default editor. When editor exits, reload the file's directory. If file was modified (check mtime), show a brief notification. | **Effort:** S

- [x] **4.7 Config hot-reloading**
  - Resolved: `:so` / `:source` calls `reload_config()` which re-parses the config file, swaps in the new `Config`, reloads theme if changed, and reloads both panes.
  - **P2** | **Files:** `src/config.rs`, `src/ui/input.rs` | **Hints:** Add `:so[urce]` command to reload config file at runtime. Watch config file with `notify` for auto-reload. | **Effort:** M

- [x] **4.8 Multiple file selection with wildcards**
  - Resolved: `*` → `Action::SelectGlob` opens an input dialog for a wildcard pattern (`*`/`?`); `Ctrl+A` → `select_all()`; `Esc` clears selection before quitting.
  - **P2** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** `*` key to select all files matching a glob pattern (input dialog). `Ctrl+A` to select all files in current pane. `Esc` to clear selection. | **Effort:** S

- [x] **4.9 Implement configurable keybindings from config file**
  - Resolved: `src/ui/keymap.rs` defines `Action` enum + `build_keymap()` which merges hardcoded defaults with `config.keybindings` overrides. All main key dispatch goes through `self.keymap`.
  - **P2** | **Files:** `src/config.rs`, `src/ui/input.rs` | **Hints:** Add `[keybindings]` section to config with string-to-action mappings. Define an `Action` enum representing every possible user action. In `input.rs`, instead of matching on `KeyCode` directly, build a `HashMap<KeyEvent, Action>` from config (with hardcoded defaults as fallback). This enables users to remap any key without code changes. Start simple: only support single-key bindings initially, expand to key sequences later. | **Effort:** M

---

## Phase 5: Infrastructure

> Goal: Tests, CI/CD, proper error handling, logging, documentation.
> Depends on: None. Should run in parallel with all phases.

### 5.1 Testing

- [x] **5.1.1 Add unit tests for `format_size()`**
  - Resolved: 8 tests covering B/KB/MB/GB/TB boundaries, fractional rounding, beyond-TB saturation.
  - **P0** | **Files:** `src/ui/panes.rs` (add `#[cfg(test)] mod tests`) | **Hints:** Test cases: 0 → "0 B", 1023 → "1023 B", 1024 → "1.0 KB", 1048576 → "1.0 MB", 1073741824 → "1.0 GB". | **Effort:** S

- [x] **5.1.2 Add unit tests for `format_date()`**
  - Resolved: 2 tests (UNIX_EPOCH, known 2024-01-15 12:30 UTC timestamp). Format is UTC, so tests are timezone-independent.
  - **P0** | **Files:** `src/ui/panes.rs` | **Hints:** Test with known `SystemTime` values. | **Effort:** S

- [x] **5.1.3 Add unit tests for `Entry::new()`**
  - Resolved: 3 tests (temp file → File, temp dir → Directory, nonexistent → Unknown with "-" fallbacks). Added `tempfile` as dev-dependency.
  - **P0** | **Files:** `src/ui/panes.rs` | **Hints:** Test with a temp file: create tempfile, construct Entry, verify kind=File, name matches, size > 0. Test with temp dir: kind=Directory. Test with nonexistent path: kind=Unknown. | **Effort:** S

- [x] **5.1.4 Add unit tests for `Pane::next_index()`**
  - Resolved: 7 tests covering wrap-around both directions, single-item, empty list, None selected.
  - **P0** | **Files:** `src/ui/panes.rs` | **Hints:** Test wrap-around (last+down=0, first+up=last), single-item list, empty list (row_count=0), None selected → 0. | **Effort:** S

- [x] **5.1.5 Add unit tests for sort logic in `read_entries()`**
  - Resolved: extracted `sort_entries()` function and added 5 tests covering Name/Size/Flagged sort types, Ascending/Descending order, and directories_on_top behavior.
  - **P1** | **Files:** `src/ui/panes.rs` | **Hints:** Extract sort to a pure function `sort_entries(entries, config) -> Vec<Entry>`. Test each SortType combination with SortOrder. Test directories_on_top ordering. | **Effort:** M

- [x] **5.1.6 Add unit tests for `Config` deserialization**
  - Resolved: added 5 tests covering default values, empty YAML (all defaults), partial YAML (merged with defaults), full YAML, and editor resolution.
  - **P1** | **Files:** `src/config.rs` | **Hints:** Test default values, test YAML parsing with partial config, test missing fields → defaults. | **Effort:** S

- [x] **5.1.7 Add unit tests for `Theme` deserialization**
  - Resolved: added 3 tests for hex color parsing (`Color::hex_to_color`) and full theme YAML deserialization. File-loading tests skipped (load_from_file calls process::exit on error).
  - **P1** | **Files:** `src/ui/theme.rs` | **Hints:** Test hex color parsing (valid, invalid, short strings). Test loading a known theme file. | **Effort:** S

- [x] **5.1.8 Add integration test: app starts and renders without panic**
  - Resolved: `tests/render.rs` drives `App::render()` into a headless `TestBackend` — populated directory (files, dirs, hidden, symlinks incl. broken), seven terminal sizes from 1x1 to 400x100, empty directory, icons on, and each overlay in turn.
  - **P1** | **Files:** `tests/integration.rs` (new) | **Hints:** Use `ratatui::Terminal::new(CrosstermBackend::new(io::sink()))` or `ratatui::backend::TestBackend`. Run one frame of `App::render()`. Assert no panic. | **Effort:** M

- [x] **5.1.9 Add integration tests for file operations (requires temp dirs)**
  - Resolved: `tests/file_ops.rs` — 7 tests over temp dirs covering copy (file + recursive tree), move (source removed), delete, rename, the capped size walk (symlinked dirs not followed) and the transfer path guards. Required exposing the crate as a library (`src/lib.rs`).
  - **P2** | **Files:** `tests/file_ops.rs` | **Effort:** L

- [x] **5.1.10 Add unit tests for dialog module (once created)**
  - Resolved: 11 tests — confirm y/n/Enter/Esc/stay-open, input typing/backspace/Shift/Ctrl-filtering/submit/cancel, message close keys.
  - **P1** | **Files:** `src/ui/dialog.rs` | **Hints:** Test confirm dialog returns correct result on y/n/Esc/Enter. Test input dialog buffers text correctly. | **Effort:** S

### 5.2 CI/CD

- [x] **5.2.1 Create GitHub Actions workflow: test, clippy, fmt**
  - Resolved: `.github/workflows/ci.yml` — stable + beta matrix, `cargo check --all-targets`, `test --all`, `clippy --all-targets -- -D warnings`, `fmt --check`. All 17 pre-existing clippy warnings were fixed and the codebase was `cargo fmt`-normalized so the strict gates pass. NOTE: the project lives on Codeberg — this workflow needs a GitHub mirror (or a Forgejo/Woodpecker port) to actually run.
  - **P1** | **Files:** `.github/workflows/ci.yml` (new) | **Hints:** Matrix: stable + beta Rust. Steps: checkout, install Rust, cache cargo, `cargo test --all`, `cargo clippy -- -D warnings`, `cargo fmt --check`. Add `cargo check` for quick validation. | **Effort:** S

- [x] **5.2.2 Add `cargo-deny` to CI for license/security auditing**
  - Resolved: `deny.toml` (licence allow-list, advisories, source pinning, targets limited to the three platforms we build) plus a CI job. Running it found and fixed a real vulnerability (crossbeam-epoch, RUSTSEC-2026-0204, via `cargo update`) and let us drop yaml-rust entirely.
  - **P2** | **Files:** `.github/workflows/ci.yml`, `deny.toml` (new) | **Hints:** `cargo install cargo-deny && cargo deny check`. Configure `deny.toml` to allow common licenses (MIT, Apache-2.0, BSD, etc). | **Effort:** S

- [x] **5.2.3 Add code coverage reporting (`cargo-llvm-cov`)**
  - Resolved: a CI job runs `cargo llvm-cov`, prints a summary and uploads `lcov.info` as an artifact — no coverage-service account needed.
  - Decided (2026-07-31): **`cargo-llvm-cov`** — it is the de-facto standard now, built on rustc's own source-based instrumentation (`-C instrument-coverage`), so it is accurate, cross-platform and works on stable. `tarpaulin` is the older ptrace-based tool, Linux/x86 only.
  - **P2** | **Files:** `.github/workflows/ci.yml` | **Hints:** `cargo install cargo-llvm-cov && cargo llvm-cov --all-features --lcov --output-path lcov.info`. Upload to codecov. Note that TUI code is largely untestable without a `TestBackend` harness (5.1.8), so expect a modest number. | **Effort:** M

- [x] **5.2.4 Add release build workflow (GitHub variant only for now)**
  - Resolved: `.github/workflows/release.yml` builds linux-x64 and both macOS targets on a `v*` tag and attaches a draft release archive that carries `themes/` alongside the binary.
  - Scope (2026-07-31): GitHub Actions only — a Forgejo/Woodpecker port for Codeberg can follow once the mirror question is settled.
  - **P2** | **Files:** `.github/workflows/release.yml` (new) | **Hints:** Trigger on tag push. Build with `--release`. Upload binary as release artifact. Consider `cargo-dist` or manual matrix for linux-x64, macos-arm64, macos-x64. | **Effort:** M

### 5.3 Error Handling

- [x] **5.3.2 Convert `main() -> io::Result<()>` to use `color_eyre::Result<()>`**
  - Resolved alongside the color-eyre quick win.
  - **P1** | **Files:** `src/main.rs` | **Hints:** `color_eyre::install()?;` at top. Change return type. This gives colorful, detailed error traces for all `?` propagations. | **Effort:** S

- [x] **5.3.3 Systematic `.unwrap()` / `.expect()` removal**
  - Resolved: the four remaining production sites are gone. Three were startup panics: the config path when no HOME exists, the fallback filesystem watcher, and two borrows inside the find-in-files walk. The only `expect` left is a literal regex compiled once in a `OnceLock`, with a message saying why it cannot fail. Scope was much smaller than the original L estimate — 0.11 had already removed the rest.
  - **P1** | **Files:** All `src/**/*.rs` | **Hints:** Convert panicking functions to return `Result`. Use `?` propagation. For truly unrecoverable errors (e.g., terminal init failure), use `.expect()` with a descriptive message. File listing errors should not crash — show error in pane body. | **Effort:** L

- [x] **5.3.4 Add error display in footer/pane for non-fatal errors**
  - Resolved: `Footer::set_status(text, is_error)` with a 3 s TTL, driven by `App::ok_status()` / `App::err_status()`. Every non-fatal failure (create, rename, delete, copy, move, theme, config save/reload, shell) and every successful operation now reports there instead of opening a modal dialog. Errors use `theme.colors.error()`, successes `info()`.
  - **P2** | **Files:** `src/ui/footer.rs`, `src/ui/input.rs` | **Effort:** M

### 5.4 Documentation

- [x] **5.4.1 Add crate-level doc comments (`//!`) to `src/lib.rs` or `src/main.rs`**
  - Resolved: `src/lib.rs` documents what rodeo is and what each top-level module (`config`, `fs`, `ui`, `cli`, `logging`) is responsible for.
  - **P2** | **Files:** `src/lib.rs` | **Effort:** S

- [x] **5.4.2 Add doc comments to all public items**
  - Resolved with a narrowed scope: every module has a `//!` block and the core types (App, Config, Theme, Colors, Pane, Panes, Entry, EntryKind, SortType/SortOrder, OpenAction) are documented. `#![warn(missing_docs)]` is deliberately *not* enabled: it reports 319 items, but most are only `pub` so integration tests can reach them, and forcing a comment onto every field produces filler.
  - **P2** | **Files:** All `src/**/*.rs` | **Hints:** Every `pub struct`, `pub fn`, `pub enum` should have `///` doc comments. Run `cargo doc --open` to verify. Enable `#![warn(missing_docs)]` once most items are documented. | **Effort:** M

- [x] **5.4.3 Update README with current status, installation instructions, keybindings**
  - Resolved: features, installation (including the theme search path), the full config file, keybinding table, commands and development commands. The old hint here was stale — it asked for `bat`/`file` runtime deps that no longer exist.
  - **P2** | **Files:** `README.md` | **Hints:** Add "Installation" section (`cargo install --path .` or `cargo build --release`). Add runtime deps section: `bat`, `file` commands (until syntect replaces bat). Add keybinding table. Link to this TODO. | **Effort:** M

- [x] **5.4.5 Choose a licence**
  - Resolved (2026-07-31): **Apache-2.0**. `LICENSE` added (text taken verbatim from a registry copy and cross-checked byte-for-byte against a second one), `license`/`description`/`repository`/`keywords`/`categories` filled in, and `publish = false` removed — `cargo package` now succeeds.
  - Found 2026-07-31 while writing the README: there is no `LICENSE` file and no `license` field in `Cargo.toml`, so the repository is implicitly all-rights-reserved despite being public. Blocks any release, and `cargo publish` refuses without it.
  - **P1** | **Files:** `LICENSE` (new), `Cargo.toml`, `README.md` | **Hints:** MIT or Apache-2.0 (or the usual dual `MIT OR Apache-2.0`) match the Rust ecosystem; GPL-3.0 if you want copyleft. Add `license = "..."` to `Cargo.toml` so the crate metadata matches. | **Effort:** S

- [x] **5.4.4 Add man page or `--help` improvement**
  - Resolved: richer `--help` (value names, config/theme locations, examples) and `docs/rodeo.1` generated from the same clap definition via `cargo run --example gen_man`, with `tests/man.rs` failing if the checked-in page drifts.
  - **P3** | **Files:** `src/cli.rs`, `docs/rodeo.1` (new) | **Hints:** Enhance clap doc strings. Optional: generate man page with `clap_mangen`. | **Effort:** S

---

## Phase 6: Pre-release UI Polish

> Goal: make rodeo look finished. Everything here came out of the 2026-07-31
> UI review — the layout wastes space on wide terminals, popups are sized by
> percentage with no cap, and the syntax colours do not match the palette.
> Depends on: nothing. All items are independent unless noted.

### 6.1 Popups

- [x] **6.1.1 Clamp popup sizes instead of pure percentages**
  - Resolved: `component::centered_popup(area, want, min, max)`. Preview 60% capped at 110x50, About sized to content (34x9 instead of 100x15), Help content-sized and packed into as many columns as the height needs — the single column silently hid six bindings at 30 rows.
  - Problem: preview is 60% w × 90% h, help 75% × 75%, about 50% × 50%. On a 200-column ultrawide that is a 120-column preview and a **150-column keybinding list** for two columns of text.
  - **P1** | **Files:** `src/ui/popup_preview.rs`, `src/ui/popup_keybinds.rs`, `src/ui/popup_about.rs` | **Hints:** Add a shared `centered_popup(area, want_w, want_h, max_w, max_h)` helper. Preview: `min(60%, ~110)` columns. Help/About: size to their actual content (longest line + padding, line count + borders) capped at ~80 columns. Keep a floor so an 80×24 terminal still works. | **Effort:** S

- [x] **6.1.2 Dim the background while a popup is open**
  - Resolved: `dim_area()` walks the frame buffer and sets the DIM attribute on everything drawn before any overlay (popups, dialogs, trash, bulk rename, find-in-files, transfer gauge).
  - Makes the popup read as a focused layer instead of a bright slab. Measured 2026-07-31: popup backgrounds already use the theme background, so the "too bright" feeling is dominance, not colour.
  - **P1** | **Files:** `src/ui/mod.rs` | **Hints:** After rendering panes and before the popup, walk `frame.buffer_mut()` over the area *outside* the popup rect and add `Modifier::DIM` (optionally fade fg toward `muted`). Ratatui has no real transparency, so this is the cheap equivalent of a scrim. | **Effort:** S

### 6.2 Syntax Highlighting Colours

- [x] **6.2.1 Build the syntect theme programmatically instead of formatting XML**
  - Resolved: a `SYNTAX_RULES` table of (selector, role, font style) becomes `ThemeItem`s directly — no XML, no parse, no panic — and the built theme is cached in `App` behind an `Arc` instead of being rebuilt per preview.
  - `to_syntect_theme()` `format!`s a ~230-line plist, parses it with syntect's plist reader and `.expect()`s the result. The background preview thread rebuilds — and re-parses — it on **every** preview open.
  - **P1** | **Files:** `src/ui/theme.rs`, `src/ui/popup_preview.rs`, `src/ui/mod.rs` | **Hints:** `syntect::highlighting::{Theme, ThemeItem, ThemeSettings, StyleModifier, ScopeSelectors}` are all public: build `Vec<ThemeItem>` from a `&[(&str scope, Role, FontStyle)]` table (~40 lines of data replacing 230 lines of XML). No runtime parse, no panic, unit-testable. Cache the built theme in `App` and rebuild it only when the theme changes. | **Effort:** M

- [x] **6.2.2 Fix the scope mapping — colours do not match the palette**
  - Resolved: scopes read out of the bundled grammars with `ParseState` instead of guessed. Keywords share one colour, type names and macros are no longer invisible, tags are not operator-coloured, punctuation is uniform. Documented that `let` and `u64` carry the identical `storage.type` scope, so declarations win that colour. 6 tests resolve concrete scope stacks so the mapping cannot regress.
  - Measured on a Rust snippet (2026-07-31): `use` is primary but `fn`/`let`/`impl`/`struct` are warning (**keywords split across two colours**); `u64`/`str`/`Self` share the keyword colour (no type distinction); `Config`/`HashMap` are **uncoloured** because the rules say `entity.name.type.*` while Rust emits `entity.name.struct`; `println!` is **uncoloured** (`support.function.macro` vs Rust's `support.macro`); punctuation is inconsistent (`:` `,` `->` muted, `;` plain).
  - **P1** | **Files:** `src/ui/theme.rs` | **Hints:** Check scopes against what the bundled Sublime grammars actually emit rather than guessing. Add missing roles: `constant.language`, `variable.language`, `support.type`, `support.class`, `entity.other.attribute-name`, `markup.heading`, `markup.list`, `meta.diff`. Keep one colour per *role* (keyword / type / function / string / number / comment / punctuation) so a file reads consistently. Note `syntect_style_to_ratatui` drops the background, so a rule that only sets a background is invisible. | **Effort:** M

### 6.3 Pane Layout

- [x] **6.3.1 Fixed-width Size and Date columns**
  - Resolved: Size is 9 cells right-aligned, the date 17 cells and muted, Name takes the remainder.
  - Columns are `Name: Fill(1), Size: 20%, Time: 30%`, so at 200 columns `550 B` is rendered in a 40-column field and everything drifts apart in whitespace.
  - **P1** | **Files:** `src/ui/panes.rs` | **Hints:** `Constraint::Length(9)` for Size (right-aligned) and `Length(16)` for the timestamp; Name takes `Fill(1)`. | **Effort:** S

- [x] **6.3.2 Full-width cursor row and a dimmed inactive pane**
  - Resolved: the cursor row is highlighted only in the focused pane, the inactive pane renders dimmed, and the reversed-yellow cell highlight is gone.
  - Only the border colour currently distinguishes the active pane.
  - **P1** | **Files:** `src/ui/panes.rs` | **Hints:** `row_highlight_style` already sets a background — make sure it spans the full row width. For the inactive pane, render entries with `muted`/`DIM` so focus is obvious at a glance. | **Effort:** S

- [x] **6.3.3 Empty and loading placeholders**
  - Resolved: `(empty directory)` and `(no matches)` are centered in the listing, with tests.
  - A directory with no entries renders as a blank box.
  - **P2** | **Files:** `src/ui/panes.rs` | **Hints:** Render a centered muted `(empty directory)` when `paths` holds only `..`, and `(no matches)` when a filter hides everything. | **Effort:** S

- [x] **6.3.4 Responsive extra columns on wide terminals**
  - Resolved: `ColumnSet::for_width()` adds git status, permissions and owner one at a time, each only while the name column can still show a reasonable name. Git status shows the two raw porcelain characters (index state in the success colour, worktree state in the warning colour) — the staged-vs-unstaged distinction colours cannot express. Owner is resolved from `/etc/passwd` (cached, numeric uid fallback), deliberately without a new dependency.
  - Now the primary answer to empty ultrawide screens, since the preview stays a popup (Open Decision 12): the spare width has to be filled with file-manager information, not preview content.
  - **P1** | **Files:** `src/ui/panes.rs` | **Hints:** Above ~120 columns per pane, add permissions (`rwxr-xr-x`), owner and a one-character git status column — the staged-vs-unstaged distinction that colours cannot express. Hide them again when narrow. | **Effort:** M

- [x] **6.3.5 File-type icons (config-gated)**
  - Resolved: `icons = true` in config.toml. Glyph by kind, then well-known name, then extension; codepoints written as `\u{...}` escapes because private-use characters get mangled in transit (the first draft lost all 41).
  - **P2** | **Files:** `src/ui/panes.rs`, `src/config.rs` | **Hints:** Nerd-font glyph per extension/kind with a `icons = true` config key (default off, since it needs a patched font). Biggest single "modern TUI" visual cue. | **Effort:** M

### 6.4 Chrome

- [x] **6.4.1 Adaptive footer labels**
  - Resolved: hints render as one line (key in the accent colour, action muted) and whole entries are dropped with an ellipsis when the terminal is narrow, instead of cutting words in half.
  - The footer truncates mid-word today (`F7 Mkdi`, `^h Hidd`).
  - **P1** | **Files:** `src/ui/footer.rs` | **Hints:** Keep short and long label variants per entry; pick per available width, and drop the lowest-priority entries instead of cutting a word in half. | **Effort:** S

- [x] **6.4.2 Useful header: breadcrumb, free space, sort/filter state**
  - Resolved: the empty middle third now shows the active path as a breadcrumb (parents muted, current directory emphasised, truncated from the left), and free space on the device sits next to the git status, sampled on navigation via `statvfs`.
  - Not done on purpose: sort/filter chips. Sort state is already the ▾/▴ arrow on the column header and an active regex filter has its own bar, so chips would only repeat what is on screen.
  - The middle third of the header currently renders an empty string.
  - **P2** | **Files:** `src/ui/header.rs` | **Hints:** Left: pane stats (as now). Middle: breadcrumb of the active path with the last segment emphasised. Right: git (as now) plus free space on the device and chips for the active sort and filter. | **Effort:** M

---

## Dependency Graph

```
Phase 0 (Bug Fixes) ─────────────────────────────────────────┐
  │                                                           │
  ├── Phase 1.1-1.2 (Sync File Ops) ──► Phase 1.3 (Vim Modes) │
  │        │                                    │              │
  │        └── Phase 1.4 (Cmd Palette) ◄────────┘              │
  │                    │                                       │
  │                    └── Phase 1.5 (Async Ops)               │
  │                                                           │
  ├── Phase 2 (Search) ────► Phase 3 (Preview Polish)          │
  │                                │                           │
  │                                └── Phase 4 (Power User)    │
  │                                                           │
  └── Phase 5 (Infrastructure) ◄── runs in parallel ──────────┘
```

**Key blocking relationships:**
- Phase 1 file ops require Phase 0 unwrap cleanup (no crashes during file operations).
- Phase 1.3 (vim modes) benefits from Phase 1.2 (sync ops) being done — modes are about triggering operations.
- Phase 1.4 (command palette) can run in parallel with 1.3.
- Phase 1.5 (async transfers) requires the `src/fs/ops.rs` extraction from Phase 1.2.
- Phase 3 syntect replacement has no blockers — can start any time.
- Phase 4 bulk rename needs file ops (1.2).
- Phase 5 runs in parallel with everything.

---

## Crate Dependencies

### Current (in `Cargo.toml`)

| Crate | Version | Used? | Notes |
|-------|---------|-------|-------|
| `chrono` | 0.4.45 | Yes | File modification time formatting |
| `clap` | 4.6.1 | Yes | CLI argument parsing |
| `color-eyre` | 0.6.5 | Yes | Error reporting in `main()` |
| `crossterm` | 0.29.0 | Yes | Terminal input events |
| `env_logger` | 0.11.10 | Yes | Logging (to be replaced by `tracing`) |
| `flate2` | 1.1.9 | Yes | Gzip decompression for `.tar.gz` preview |
| `ignore` | 0.4.31 | Yes | Recursive walking for find-in-files (respects .gitignore) |
| `image` | 0.25.10 | Yes | Image preview decoding |
| `infer` | 0.16 | Yes | File type detection (replaced `file` command) |
| `log` | 0.4.31 | Yes | Log macros (to be replaced by `tracing`) |
| `notify` | 8.2.0 | Yes | Filesystem event watching (live pane refresh) |
| `nucleo` | 0.5.0 | Yes | Fuzzy file matching |
| `pdf-extract` | 0.12.0 | Yes | PDF text extraction for preview |
| `ratatui` | 0.30.0 | Yes | Core TUI framework |
| `ratatui-image` | 11.0.4 | Yes | Terminal image rendering |
| `regex` | 1.13.1 | Yes | Regex filter (find-in-files reuse planned) |
| `serde` | 1.0.228 | Yes | Config/theme deserialization |
| `syntect` | 5.3.0 | Yes | Syntax highlighting (replaced `bat`) |
| `tar` | 0.4.46 | Yes | Tar archive listing for preview |
| `tempfile` | 3 | Yes | Dev-dependency: temp dirs for unit/integration tests |
| `trash` | 5.2.6 | Yes | Delete to system trash (with permanent fallback) |
| `xdg` | 3.0.0 | Yes | XDG base directories for config path |
| `toml` | 1.1.4 | Yes | Config + theme parsing (replaced `yaml_serde`) |
| `zip` | 8.6.0 | Yes | Zip archive listing for preview (deflate only) |

### New Crates Needed (by priority)

| Crate | Version | Phase | Purpose |
|-------|---------|-------|---------|
| `gix` | 0.70 | Phase 4 | Pure-Rust git status (replace `git` CLI) |

> **Note:** Crate versions above are current as of 2026-06-19. Check [crates.io](https://crates.io) for latest versions before adding. Use `cargo add <crate>` for automatic version resolution.

### Crates to Remove

| Crate | Reason |
|-------|--------|
| — | None. `log` + `env_logger` are staying (see Open Decision 11). |

---

## Open Decisions

| # | Question | Options | Impact | Recommendation |
|---|----------|---------|--------|----------------|
| 3 | **Vim-modal or modeless?** | Modal (Normal/Command/Visual) or single-mode with key combos | Fundamental UX design. Affects all keybinding code. | **Modal** — all three planning docs agree. Vim users are the target audience. Start modeless in Phase 1.2, introduce modes in 1.3. |
| 4 | **`syntect` vs keep `bat`?** | `syntect` (pure Rust, bundled) vs `bat` (external binary) | Preview architecture, binary size, runtime deps | **`syntect`** — all planning docs recommend it. Removes external dependency, works offline, gives full control over theme mapping. `bat` was a quick prototype shortcut. |
| 5 | **`gix` vs `git` CLI?** | `gix` (pure Rust) vs shelling out to `git` binary | Git status performance, binary size, portability | **`gix`** — removes runtime dependency on `git` binary, no shell overhead, consistent behavior. But lower priority: current `git` CLI approach works fine for header stats. |
| 6 | **Single crate or workspace?** | Single crate vs `rodeo-core` + `rodeo-tui` | Build complexity, compile times, API boundaries | **Single crate** for now — project is small (~1600 lines). Split when it hurts: when adding SFTP, or when a library API is needed. All planning docs agree. |
| 7 | **Keybinding customization?** | Hardcoded vs configurable in `config.toml` | User experience, code complexity | **Configurable** (eventually) — start hardcoded, but design the input system to accept a keybinding map. Add to config after Phase 1.3 when the keybinding surface stabilizes. |
| 8 | **Linux-first or cross-platform from day 1?** | Linux only vs Linux + macOS + Windows | Dependency choices, testing burden | **Linux-first** with cross-platform awareness. Use cross-platform crates (`crossterm`, `notify`, `trash`). Don't test on macOS/Windows initially, but avoid platform-specific code. |
| 9 | **Trash: permanent delete fallback?** | Only trash (fail on unsupported FS) vs fallback to `rm` | Safety vs. functionality | **Trash with permanent fallback** — if `trash` crate fails (network FS, some Linux configs), show a prominent "Trash unavailable. Permanently delete?" confirmation with red styling. |
| 10 | **Single binary or multiple?** | One `rodeo` binary vs `rodeo` + `rodeo-server` for remote | Distribution simplicity vs. capability | **Single binary** — remote filesystem features are Phase 6+. No server mode needed now. |
| 12 | ~~**Persistent preview pane or popup?**~~ → **DECIDED: popup only** | Third column following the cursor vs the existing modal popup | Layout, preview cost per keystroke | **Popup only** (2026-07-31): rodeo is a file manager first — the two panes exist so you can work *between* two directories, and a preview column competes with that. A persistent pane would also turn a user-initiated load into one per cursor movement (PDF extraction, directory size walks), needing debouncing, a load policy and a cache for something that is not the point of the tool. Wide terminals get filled with file-manager information instead (6.3.4, 6.4.2). |
| 11 | ~~**`tracing` or `log`?**~~ → **DECIDED: keep `log`** | `tracing` + `tracing-subscriber` vs `log` + `env_logger` | Logging stack, ~50 call sites | **`log` + `env_logger`** (2026-07-31): `tracing` earns its keep in async services and libraries where cross-task spans matter. rodeo is a single-threaded TUI with one worker thread, and `log` is still fully idiomatic for that. Revisit only if concurrency grows or log rotation (`tracing-appender`) becomes necessary. |

---

## Summary: Effort Estimates by Phase

| Phase | Done | Open | Notes |
|-------|------|------|-------|
| Quick Wins | 12 | 0 | — |
| Phase 0: Bug Fixes | 21 | 0 | — |
| Phase 1: File Ops | 22 | 0 | — |
| Phase 2: Search | 5 | 0 | — |
| Phase 3: Preview | 12 | 0 | — |
| Phase 4: Power User | 8 | 0 | — |
| Phase 5: Infrastructure | 22 | 0 | complete |
| Phase 6: Pre-release UI Polish | 11 | 0 | complete |
| **Total** | **113** | **0** | |

---

## Legend

- **P0**: Critical — crash, data loss, broken core feature. Fix before anything else.
- **P1**: High — important feature, major UX gap, blocks other work.
- **P2**: Medium — nice to have, polish, non-blocking improvement.
- **P3**: Low — future enhancement, optional.

- **S**: Small — hours (< 4 hours)
- **M**: Medium — days (1-3 days)
- **L**: Large — weeks (1-2 weeks)
- **XL**: Extra Large — months
