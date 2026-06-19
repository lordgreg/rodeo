# rodeo — Development TODO

> **Last updated:** 2026-06-19
> **Current state:** Dual-pane navigation + preview + theming + git header. No file operations, no search, no tests.

| Phase | README Milestone | Focus |
|-------|-----------------|-------|
| Phase 0 (Stability) | — | Pre-M2 bug fixes, crash fixes, dead code removal |
| Phase 1 (File Ops) | M2 | Copy, move, delete, rename, mkdir, dialogs, command palette |
| Phase 2 (Search) | M3 | Fuzzy search, regex filter, find-in-files |
| Phase 3 (Preview) | M4 | Syntax highlighting, archive preview, bookmarks, file watching |
| Phase 4 (Power User) | M5 | Bulk rename, trash, git column, shell commands, directory sizes |
| Phase 5 (Infrastructure) | — | Tests, CI/CD, error handling, logging, docs |

---

## Quick Wins *(< 30 min each)*

These are small, self-contained fixes with high impact-to-effort ratio. Do them first to build momentum.

- [ ] **Fix footer lies (Also tracked as 0.6):** Replace "x Cut"/"p Paste" with actual bindings ("x Select", "Space Preview", "Tab Switch", "q Quit", "? About")
  - **P0** | **Files:** `src/ui/footer.rs:18-24` | **Hints:** Change the `keymaps` vec strings to match real bindings in `input.rs`. | **Effort:** S

- [ ] **Remove `active_keybind_popup` dead code (Also tracked as 0.12):** Flag is never set to `true` — popup can never appear. Either wire it to a keybinding or delete the field + render call.
  - **P1** | **Files:** `src/ui/uiconfig.rs:14`, `src/ui/mod.rs:87-89`, `src/ui/input.rs:33,40,149,153,165` | **Hints:** If keeping, bind `F1` or `K` to toggle it. If removing, delete the field, the render block in `mod.rs`, and all references in `input.rs`. | **Effort:** S

- [ ] **Fix `FileOpened` empty match arm (Also tracked as 0.5):** Enter on a file returns `FileOpened(_entry)` but the match arm at `input.rs:126` is empty `{}`. Open the file with `$EDITOR`/`$VISUAL`.
  - **P0** | **Files:** `src/ui/input.rs:126` | **Hints:** `std::process::Command::new(env::var("EDITOR").unwrap_or("vim".into())).arg(&entry.path).status()`. Need `use std::env;`. | **Effort:** S

- [ ] **Fix `ReadOnly` config unused (Also tracked as 0.13):** `config.read_only` field parsed from YAML but never checked. Gate file operations (cut/copy/delete/rename) behind it.
  - **P1** | **Files:** `src/config.rs:46` (field), `src/ui/input.rs` (check before ops) | **Hints:** Add `pub fn is_read_only(&self) -> bool` to `Config`. Check in `handle_main_key` before allowing destructive operations (once implemented). | **Effort:** S

- [ ] **Remove `color-eyre` from Cargo.toml or use it:** Dependency declared but never imported. Either integrate it (replace `env_logger` with `color-eyre` + `tracing`) or drop it to reduce compile times.
  - **P1** | **Files:** `Cargo.toml:10` | **Hints:** If keeping, add `color_eyre::install()?;` at top of `main()`. If dropping, change `io::Result<()>` to use `eyre::Result` separately. See Phase 5 for full error handling plan. | **Effort:** S

- [ ] **Populate About popup:** Currently renders an empty titled block. Add app name, version, author, and repository link.
  - **P2** | **Files:** `src/ui/popup_about.rs` | **Hints:** Add a `Paragraph` inside the block with app info. Use `env!("CARGO_PKG_VERSION")` for version. Center-text align inside the block. | **Effort:** S

- [ ] **Populate Keybinds popup:** Currently renders an empty titled block. List actual keybindings from `input.rs`.
  - **P2** | **Files:** `src/ui/popup_keybinds.rs` | **Hints:** Create a static `&[(&str, &str)]` mapping key to description. Render as a two-column table or list. Wire popup toggle to a keybinding (e.g., `F1` or `K`) if keeping `active_keybind_popup`. | **Effort:** S

- [ ] **Fix header info placeholder:** Header hardcodes `"~info one"` in `App::new()`. Show active pane name or total file count.
  - **P2** | **Files:** `src/ui/mod.rs:49` | **Hints:** Replace with useful metric: total files in active directory, app version string, or remove the field entirely. The Header is a single shared widget (not per-pane), so pane-specific info belongs elsewhere. | **Effort:** S

- [ ] **Add `README_ONLY` marker for config:** Add `read_only` getter to `Config` and document the field in the default config template.
  - **P2** | **Files:** `src/config.rs` | **Hints:** `pub fn is_read_only(&self) -> bool { self.read_only }`. Add a comment in default config generation. | **Effort:** S

- [ ] **Add `.unwrap()` audit comment markers:** Mark every `.unwrap()` / `.expect()` with `// TODO: proper error handling` for later systematic replacement.
  - **P1** | **Files:** `src/**/*.rs` | **Hints:** `rg '\.unwrap\(\)' src/` shows ~16 instances. `rg '\.expect(' src/` shows ~6 more. | **Effort:** S

- [ ] **Fix Cargo.toml edition:** `edition = "2024"` has been stable since Rust 1.85. Confirm minimum supported Rust version and adjust edition accordingly.
  - **P1** | **Files:** `Cargo.toml:4` | **Hints:** Check `rustup show`. If stable, change to `"2021"`. | **Effort:** S

- [ ] **Fix `println!` debug statement in `main.rs:27`:** `println!("Config: {:?}", config);` leaks to terminal. Replace with `info!` or remove.
  - **P2** | **Files:** `src/main.rs:27` | **Hints:** `info!("Config: {:?}", config);` | **Effort:** S

- [ ] **Add `.gitignore` entries:** Missing `*.swp`, `*.swo`, `.*.swp`, `*.log`, `/themes/*.swp`.
  - **P2** | **Files:** `.gitignore` | **Hints:** Standard Rust + editor ignore patterns. | **Effort:** S

- [ ] **Bind `Ctrl+L` to manual redraw/refresh**
  - **P2** | **Files:** `src/ui/input.rs:67-77` | **Hints:** In `handle_ctrl_key`, add `KeyCode::Char('l') => { self.panes.reload(&self.config, false); return true; }`. This forces a re-read of both panes' directories and a full terminal redraw. Essential when terminal state gets corrupted. | **Effort:** S

- [ ] **Fix README M0 workspace claim**
  - **P2** | **Files:** `README.md:9`, `Cargo.toml` | **Hints:** README M0 checklist says `[x] Project scaffolding with Cargo workspaces` but the project is a single crate (no `[workspace]` section). Either add a workspace to Cargo.toml (per planning docs' suggestion of `rodeo-core` + `rodeo-tui`) or update the README to say `single crate` instead of `workspaces`. | **Effort:** S

---

## Phase 0: Bug Fixes & Code Health

> Goal: Stop crashing. Remove dead code. Make existing features work correctly.
> Dependencies: None. Everything here can start immediately.

- [ ] **0.1 Fix crash: unknown Ctrl keys call `todo!()` → PANIC**
  - **P0** | **Files:** `src/ui/input.rs:75` | **Hints:** Replace `_ => todo!("no action defined while pressing CTRL")` with `_ => return false`. Unknown Ctrl combos should be a no-op, not a crash. Use `log::debug!("unhandled Ctrl+{:?}", key.code)`. | **Effort:** S

- [ ] **0.2 Fix crash: unknown Shift keys call `unimplemented!()` → PANIC**
  - **P0** | **Files:** `src/ui/input.rs:112` | **Hints:** Replace `_ => unimplemented!("SHIFT key {} not yet implemented.", key.code)` with `_ => return false`. Log the unhandled key. | **Effort:** S

- [ ] **0.3 Fix crash: preview of non-file items calls `todo!()` → PANIC**
  - **P0** | **Files:** `src/ui/input.rs:170` | **Hints:** Replace `_ => todo!("Preview of non-files not implemented yet")` with graceful handling. For `EntryKind::Parent`, show "Cannot preview parent directory". For `EntryKind::Unknown`, show "Unknown file type — cannot preview". Use `log::warn!`. | **Effort:** S

- [ ] **0.4 Fix `handle_popup_key` always returns `true` (consumes all keys when popup active)**
  - **P0** | **Files:** `src/ui/input.rs:38-65` | **Hints:** The fallthrough `true` at line 64 means ANY key when a popup is active gets consumed — including `Esc` after it was already handled. Change the final `true` to `false` (return false = didn't handle, let main handler try). Test with `Esc` in preview popup: it should close preview, not quit. | **Effort:** S

- [ ] **0.5 Fix Enter on file does nothing (empty `FileOpened` arm)**
  - **P0** | **Files:** `src/ui/input.rs:126` | **Hints:** (See also Quick Wins above — this is so critical it appears in both lists.) Spawn `$EDITOR` with the file path. On editor exit, reload pane to reflect any changes. Use `std::env::var("EDITOR").unwrap_or_else(|_| "vi".into())`. | **Effort:** S

- [ ] **0.6 Fix footer: replace misleading keybindings with true ones**
  - **P0** | **Files:** `src/ui/footer.rs:18-24` | **Hints:** (See also Quick Wins above.) Current lies: `x Cut` (x=toggle select), `p Paste` (p=unbound), `SPACE select` (SPACE=preview), `? preview` (?=about). Replace with `h/j/k/l Move`, `Enter Open`, `Tab Switch`, `x Select`, `Space Preview`, `q Quit`, `? About`. Make Ctrl+h and Backspace visible too. | **Effort:** S

- [ ] **0.7 Fix preview: `self.selected.as_ref().unwrap()` panics if selected is None at render time**
  - **P1** | **Files:** `src/ui/popup_preview.rs:119` | **Hints:** If the entry was deleted between opening preview and rendering, the `unwrap()` crashes. Change to `let Some(entry) = self.selected.as_ref() else { return; };`. | **Effort:** S

- [ ] **0.8 Document popup key precedence chain in `handle_popup_key`**
  - **P1** | **Files:** `src/ui/input.rs:38-77` | **Hints:** The current precedence order (popup keys → ctrl keys → shift keys → main keys) is correct. Ctrl+Up/Down in preview work because they return `true` before `handle_ctrl_key` runs. Add a comment at the top of `handle_popup_key` documenting the chain: "Keys checked in order: popup-specific → Ctrl-modified → Shift-modified → unmodified. Popup handler takes priority when any popup is active." | **Effort:** S

- [ ] **0.9 Make popups dismissible with `q` and `Esc` consistently**
  - **P1** | **Files:** `src/ui/input.rs:38-65,139-160` | **Hints:** In `handle_popup_key`, add `KeyCode::Char('q')` alongside `Esc` to close all popups. Mirror `Esc` behavior: if popups active, close them; otherwise quit. | **Effort:** S

- [ ] **0.10 Add `g`/`G`/`gg` Vim-style jump navigation**
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** `gg` = jump to top of file list, `G` = jump to bottom, `g` = prefix for other motions (future). Add `Pane::goto_first()` and `Pane::goto_last()` methods that set `self.state.select(Some(0))` and `self.state.select(Some(max))` respectively. In `handle_main_key`, match `KeyCode::Char('g')` — since `g` is a prefix, you'll need a small state machine: first `g` sets a flag, second `g` triggers `goto_first()`. Same for `G` (single key). | **Effort:** S

- [ ] **0.11 Audit and tag all `.unwrap()` / `.expect()` calls for systematic replacement**
  - **P1** | **Files:** `src/config.rs:105, 114, 123, 128, 131, 133, 134, 136, 140`, `src/ui/panes.rs:139,263,291,393`, `src/ui/popup_preview.rs:89,92,94,147,153-154`, `src/ui/theme.rs:133,136,163` | **Hints:** Count: ~22 unwrap/expect sites. Tag each with `// TODO(#error-handling):`. Replace with `?` operator after converting functions to return `Result`. Most panics happen on: filesystem errors (permissions, missing files), `file` command failures, image decode failures. | **Effort:** M

- [ ] **0.12 Remove dead code: `active_keybind_popup` never activated**
  - **P1** | **Files:** See Quick Wins above. | **Hints:** (See also Quick Wins above.) Note: `active_keybind_popup` is only ever set to `false` in the codebase. The `?` key sets it to `false` (line 149), and Esc sets it to `false` (line 40). It is never set to `true` — so the popup can never appear. Either bind to key (recommend `F1` or `K`) with populated content, or remove: `uiconfig.rs:14`, `mod.rs:87-89`, `input.rs` references at lines 33,40,149,153,165. | **Effort:** S

- [ ] **0.13 Remove dead code: `Config::read_only` never checked**
  - **P1** | **Files:** `src/config.rs:46`, `src/ui/input.rs` | **Hints:** (See also Quick Wins above.) Add getter. Check in `handle_main_key` before allowing: cut/copy/delete/rename (once implemented), toggle_select, Enter-on-file. Show a status message "Read-only mode — operation blocked" in footer. | **Effort:** S

- [ ] **0.14 Fix config format mismatch: code uses YAML, README/planning docs say TOML**
  - **P2** | **Files:** `src/config.rs`, `Cargo.toml` | **Hints:** Decision needed: YAML or TOML? (See Open Decisions.) If staying YAML: update README and planning docs. If switching to TOML: swap `yaml_serde` for `toml`, update `config.yaml` → `config.toml`, update `CONFIG_FILENAME`. TOML is more Rust-idiomatic and recommended by planning docs. | **Effort:** M (if switching)

- [ ] **0.15 Fix `Entry::parent()`: `canonicalize().unwrap()` panics if parent doesn't exist or permissions deny**
  - **P1** | **Files:** `src/ui/panes.rs:139` | **Hints:** `PathBuf::from(dir).join("..")` then `canonicalize()` — if at filesystem root, `..` is still root (no panic). But permission errors or deleted directories will crash. Use `.ok()?` fallback: return an Entry with `kind: Parent` and the joined path (canonicalization is a nice-to-have). | **Effort:** S

- [ ] **0.16 Fix `read_entries()`: `fs::read_dir(dir).unwrap()` panics on permission denied or missing dir**
  - **P1** | **Files:** `src/ui/panes.rs:393` | **Hints:** Use `match fs::read_dir(dir) { Ok(rd) => rd, Err(e) => { log::error!("Cannot read {}: {}", dir, e); return vec![Entry::parent(dir)]; }}`. | **Effort:** S

- [ ] **0.17 Fix `Pane::open()` canonicalize unwrap can panic**
  - **P1** | **Files:** `src/ui/panes.rs:291-292` | **Hints:** `entry.path.canonicalize().unwrap()` — permissions or broken symlinks crash. Use `match entry.path.canonicalize() { Ok(p) => ..., Err(e) => { log::error!(...); return OpenAction::Nothing; }}`. | **Effort:** S

- [ ] **0.18 Check `handle_main_key` `Esc` logic: quits app if no popups active**
  - **P1** | **Files:** `src/ui/input.rs:152-160` | **Hints:** This means `Esc` without any popup quits the app. MC tradition uses `F10` or `q` for quit. This is a design choice — confirm it's intentional. Recommendation: make Esc a no-op when no popups are open, to prevent accidental exits. Only `q` (and `:q`) should quit. | **Effort:** S

- [ ] **0.19 Prevent preview of `..` (parent entry)**
  - **P2** | **Files:** `src/ui/input.rs:161-173` | **Hints:** Currently `Space` on `..` tries to preview — `EntryKind::Parent` falls into the `todo!()` crash in 0.3. After fixing 0.3, still show a "Cannot preview parent directory" message rather than an error. | **Effort:** S

- [ ] **0.20 Add runtime dependency documentation**
  - **P1** | **Files:** `README.md` | **Hints:** Document that `bat` and `file` commands must be installed for preview to work. Previews silently degrade without them. At startup, check `which bat` and `which file`; if missing, show a one-time warning in the footer. Previews silently fail without bat. | **Effort:** S

- [ ] **0.21 Sanitize header directory display for very long paths**
  - **P2** | **Files:** `src/ui/panes.rs:383` | **Hints:** `self.path.to_string()` is used as pane title. Very long paths overflow the pane border. Truncate with `...` prefix: `format!("...{}", &path[path.len().saturating_sub(width)..])`. | **Effort:** S

---

## Phase 1: File Operations (M2)

> Goal: Copy, move, delete, rename, mkdir — the core of a file manager.
> Depends on: Phase 0 completed (no crashes, unwrap cleanup).
> Key design: Vim-style modal keybindings (y=yank, d=cut, p=paste, dd=delete, r=rename).

### 1.1 Confirmation Dialog Infrastructure

- [ ] **1.1.1 Create `src/ui/dialog.rs` — generic confirmation popup module**
  - **P0** | **Files:** `src/ui/dialog.rs` (new), `src/ui/mod.rs` | **Hints:** A modal dialog that displays a message (e.g., "Delete 3 files?") with Yes/No options. Take a title string, message string, and two callbacks or return a `ConfirmResult` enum. Render as centered block with borders, keyboard navigation (y/n or Enter/Esc). Add `active_dialog: Option<Dialog>` to `UiConfig`. | **Effort:** M

- [ ] **1.1.2 Implement `Dialog` with actions: confirm, confirm_multi, input (for rename/mkdir)**
  - **P0** | **Files:** `src/ui/dialog.rs` | **Hints:** Three variants: `Confirm { title, message }`, `Input { title, prompt, value }`, `Message { title, text }`. Use an enum `DialogKind`. Render input dialogs with a cursor and single-line text field. | **Effort:** M

- [ ] **1.1.3 Add dialog input handling in `App::handle_input`**
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/mod.rs` | **Hints:** If dialog is active, route keys to dialog handler (Enter=confirm, Esc=cancel, for input dialogs: backspace=delete, text=append). Dialogs take priority over all other key handling. | **Effort:** M

- [ ] **1.1.4 Add dialog rendering in `App::render`**
  - **P0** | **Files:** `src/ui/mod.rs:70-104` | **Hints:** After popups, render dialog as a centered clear+block+paragraph. Dialogs should render on top of everything. | **Effort:** M

### 1.2 File Operations — Synchronous (Phase 1a)

- [ ] **1.2.1 Implement file copy (single file)**
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** Vim binding: `y` yanks (copies path to clipboard buffer), `p` pastes. Non-modal for now: `F5` or `Ctrl+C` copies. Implementation: `std::fs::copy(src, dst)`. Show confirmation if destination exists. Copy the `Entry` from active pane to inactive pane's directory. | **Effort:** M

- [ ] **1.2.2 Implement file move (single file)**
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** Vim binding: `d` cuts (moves path to clipboard buffer), `p` pastes. Non-modal: `F6`. Implementation: `std::fs::rename(src, dst)`. Falls back to copy+delete for cross-device moves. Show confirmation. | **Effort:** M

- [ ] **1.2.3 Implement file delete (single file, to trash)**
  - **P0** | **Files:** `src/ui/input.rs`, `Cargo.toml` | **Hints:** Use `trash` crate. Add `trash = "5"` to Cargo.toml. Binding: `dd` (or `Delete` key). Show confirmation dialog: "Move 'filename' to trash?". If trash fails (e.g., on some filesystems), offer permanent delete as fallback with extra confirmation. | **Effort:** M

- [ ] **1.2.4 Implement file rename (inline or dialog)**
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/dialog.rs` | **Hints:** Binding: `r` on a file. Open an input dialog pre-filled with the current filename. On confirm, `std::fs::rename(old, new)`. Reload pane afterward. Handle errors: name collision, invalid chars, permissions. | **Effort:** M

- [ ] **1.2.5 Implement mkdir**
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/dialog.rs` | **Hints:** Binding: `F7` or `Ctrl+N`. Open input dialog with prompt "Directory name:". Create with `std::fs::create_dir()`. Reload pane. Handle errors: already exists, permission denied. | **Effort:** S

- [ ] **1.2.6 Implement touch (create empty file)**
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/dialog.rs` | **Hints:** Binding: `Ctrl+T` or similar. Input dialog for filename. Create with `std::fs::File::create()`. Reload pane. | **Effort:** S

- [ ] **1.2.7 Add selection-aware batch operations**
  - **P1** | **Files:** `src/ui/panes.rs`, `src/ui/input.rs` | **Hints:** When multiple files are selected (via `x`), operations apply to all selected files. For copy/move, the target is the other pane's directory. For delete, show "Delete N files?". Gather selected entries: `pane.paths.iter().filter(|e| e.selected).collect()`. | **Effort:** M

- [ ] **1.2.8 Update footer to show contextual keybindings**
  - **P1** | **Files:** `src/ui/footer.rs` | **Hints:** After implementing operations, show actual bindings: `y Yank`, `d Cut`, `p Paste`, `r Rename`, `dd Delete`, `Ctrl+n Mkdir`, `Ctrl+t Touch`, `x Select`. Make footer dynamic: when files are selected, show selection count and bulk-action hint. | **Effort:** S

### 1.3 Vim-Style Modal Keybindings (Phase 1b)

- [ ] **1.3.1 Add mode state machine: Normal, Command, Visual**
  - **P1** | **Files:** `src/ui/uiconfig.rs` or new `src/ui/mode.rs` | **Hints:** `enum Mode { Normal, Command, Visual, Insert }`. `Normal` is default (navigation). `Command` is `:` palette. `Visual` is selection mode (entered via `v`). Track in `UiConfig` or `App`. Render mode indicator in footer: `-- NORMAL --`, `-- VISUAL --`, `-- COMMAND --`. | **Effort:** M

- [ ] **1.3.2 Implement `v` → enter visual (selection) mode**
  - **P1** | **Files:** `src/ui/input.rs` | **Hints:** In visual mode, `j`/`k` move cursor AND toggle selection of traversed files. `V` for line-wise (toggle all visible). `Esc` to exit visual mode. Operations (`d`, `y`, `r`) in visual mode operate on all selected files. | **Effort:** M

- [ ] **1.3.3 Implement yank (`y`) and put (`p`) clipboard**
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** Internal clipboard: `Vec<PathBuf>` in `App` or `UiConfig`. `y` yanks selected (or current) file path to clipboard. `p` pastes (copies) from clipboard to current pane directory. `P` for move (cut+paste). Show clipboard count in footer: `"[2 files yanked]"`. | **Effort:** M

- [ ] **1.3.4 Implement `dd` (delete current file or all selected)**
  - **P1** | **Files:** `src/ui/input.rs` | **Hints:** In Normal mode, `dd` = delete current file (with confirmation). In Visual mode, `d` = delete all selected files (with confirmation). | **Effort:** M

- [ ] **1.3.5 Implement `r` (rename current file)**
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/dialog.rs` | **Hints:** In Normal mode, `r` opens rename dialog for the currently highlighted file. In Visual mode, prompt whether to rename individually or enter bulk rename mode. | **Effort:** M

### 1.4 Command Palette

- [ ] **1.4.1 Create command palette popup (`:` key)**
  - **P1** | **Files:** `src/ui/popup_cmd.rs` (new), `src/ui/mod.rs` | **Hints:** Press `:` to open a command input bar at bottom of screen (or centered popup). Model after Vim's command line. Commands: `:q` quit, `:w` save config, `:e <path>` navigate, `:mkdir <name>`, `:touch <name>`, `:delete`, `:rename <new>`, `:cd <path>`, `:theme <name>`, `:help`. Auto-complete commands with tab. | **Effort:** M

- [ ] **1.4.2 Implement command parser and dispatcher**
  - **P1** | **Files:** `src/ui/popup_cmd.rs` | **Hints:** Parse `:command [args...]`. Use a match on the command name. Show "Unknown command: foo" for unrecognized input. Reference: `xplr` and `lf` command syntax. | **Effort:** M

- [ ] **1.4.3 Add `:!<shell command>` runner**
  - **P2** | **Files:** `src/ui/popup_cmd.rs` | **Hints:** `:!ls -la` runs a shell command and shows output in a scrollable popup or preview pane. Use `std::process::Command::new("sh").args(["-c", cmd])`. Suspend UI during execution, capture stdout/stderr, display result. Add `:shell` to spawn an interactive subshell. | **Effort:** M

### 1.5 Async Operations + Progress (Tokio-dependent)

- [ ] **1.5.1 Add `tokio` to dependencies**
  - **P1** | **Files:** `Cargo.toml` | **Hints:** `tokio = { version = "1", features = ["full"] }` or minimal: `["rt-multi-thread", "fs", "sync", "macros"]`. Only needed for progress bars + cancel support. | **Effort:** S

- [ ] **1.5.2 Implement async file copy with progress bar**
  - **P1** | **Files:** `src/fs/ops.rs` (new), `src/ui/dialog.rs` | **Hints:** When copying large files (>10MB) or multiple files, spawn a dialog with a progress bar. Use `tokio::fs::copy` or stream chunks manually for progress tracking. Cancel via `Esc`. Progress = bytes_copied / total_bytes. Update bar at ~30fps via `tokio::select!` with `tokio::time::interval`. Realistic scope: integrating tokio into a previously synchronous codebase, streaming chunked copy, progress UI at 30fps, cancel support, and tokio-ratatui event loop integration is ~2-4 weeks for a first-time tokio integration. | **Effort:** XL

- [ ] **1.5.3 Implement async file move with progress**
  - **P1** | **Files:** `src/fs/ops.rs` | **Hints:** Try `rename` first (instant). If `EXDEV` (cross-device), fall back to copy+delete with progress bar. | **Effort:** M

- [ ] **1.5.4 Create `src/fs/mod.rs` and `src/fs/ops.rs` — extract file operation logic**
  - **P1** | **Files:** `src/fs/mod.rs` (new), `src/fs/ops.rs` (new) | **Hints:** Move file operation implementations out of UI code. Functions: `copy_file(src, dst) -> Result`, `move_file(src, dst) -> Result`, `delete_file(path) -> Result`, `copy_dir(src, dst) -> Result` (recursive). Keep UI code thin. | **Effort:** M

---

## Phase 2: Search & Filter (M3)

> Goal: Find files fast. Fuzzy search, regex filter, search file contents.
> Depends on: Phase 0. Phase 1 is not strictly required but helps (command palette infrastructure).

- [ ] **2.1 Add `nucleo` for fuzzy matching**
  - **P0** | **Files:** `Cargo.toml` | **Hints:** `nucleo = "0.5"`. Lightweight, no large deps. Alternative: `skim` (fzf-compatible but heavier). Planning docs recommend `nucleo`. | **Effort:** S

- [ ] **2.2 Implement fuzzy file finder (`/` key)**
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/search.rs` (new), `src/ui/mod.rs` | **Hints:** Press `/` to open a search bar at bottom of the active pane (or as a popup). Type to fuzzy-filter entries in the current directory. Results update in real-time as you type. `Esc` to cancel, `Enter` to select top match. Use `nucleo::Matcher` with the file names. The pane should highlight matching characters in filenames. | **Effort:** M

- [ ] **2.3 Implement regex filter (`Ctrl+F`)**
  - **P0** | **Files:** `src/ui/input.rs`, `src/ui/search.rs` | **Hints:** Press `Ctrl+F` to open a regex input bar. Type a regex pattern to filter directory listing. Only files matching the regex are shown. Invalid regex shows error in the bar. Use the `regex` crate: `regex = "1"`. Add to `Cargo.toml`. | **Effort:** M

- [ ] **2.4 Implement find-in-files (search file contents)**
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/search.rs` | **Hints:** Press `Ctrl+G` or similar to open "grep" mode. Enter search term. Walk directory tree with `ignore` crate (respects .gitignore). Search each file with `ripgrep`-style line matching. Show results in a new pane or popup: `filename:line: match`. Allow `Enter` on a result to open the file at that line. Add `ignore = "0.4"` to `Cargo.toml`. | **Effort:** L

- [ ] **2.5 Add search history persistence**
  - **P2** | **Files:** `src/ui/search.rs`, `src/config.rs` | **Hints:** Store last 100 search queries in config or a separate history file. Use `Up`/`Down` in search bar to cycle through history. Save on quit, load on startup. | **Effort:** S

- [ ] **2.6 Highlight search matches in file listing**
  - **P1** | **Files:** `src/ui/panes.rs`, `src/ui/search.rs` | **Hints:** When filter is active, render matching filename characters with a highlight color (e.g., yellow background or bold). Use ratatui `Span` with style for matched portion of filename. | **Effort:** M

- [ ] **2.7 Add `--search` and `--regex` CLI flags for headless use**
  - **P2** | **Files:** `src/cli.rs`, `src/main.rs` | **Hints:** `-s/--search <PATTERN>` opens with search pre-filled. `-r/--regex <PATTERN>` opens with regex filter pre-applied. Mentioned in README M0 checklist. | **Effort:** S

---

## Phase 3: Preview & Polish (M4)

> Goal: Rich previews, remove external dependencies, add bookmarks and history.
> Depends on: Phase 0. Phase 2 (search) is recommended for find-in-files preview integration.

### 3.1 Syntax Highlighting — Replace `bat` with `syntect`

- [ ] **3.1.1 Add `syntect` to dependencies**
  - **P0** | **Files:** `Cargo.toml` | **Hints:** `syntect = { version = "5", default-features = false, features = ["default-fancy"] }`. Sublime Text grammars bundled at compile time. No external binary needed. | **Effort:** S

- [ ] **3.1.2 Implement syntax-highlighted text preview with syntect**
  - **P0** | **Files:** `src/ui/popup_preview.rs` | **Hints:** Use `syntect::easy::HighlightLines` which outputs `Vec<(Style, &str)>` directly — more efficient than HTML round-trip. Create a `SyntaxSet` and `ThemeSet` once (lazy_static or load at startup). For each file, detect language via extension using `SyntaxSet::find_syntax_by_extension`, then iterate `HighlightLines` to produce styled spans for ratatui. Fall back to plain text. This removes the `bat` runtime dependency entirely. | **Effort:** M

- [ ] **3.1.3 Add theme-aware syntax highlighting**
  - **P1** | **Files:** `src/ui/popup_preview.rs`, `src/ui/theme.rs` | **Hints:** Map syntect's theme colors to rodeo's current theme. Or bundle a rodeo-specific Sublime Text theme. Use background/fg colors that work with the active rodeo theme. | **Effort:** M

- [ ] **3.1.4 Implement runtime theme switching**
  - **P2** | **Files:** `src/ui/input.rs`, `src/ui/theme.rs`, `src/ui/mod.rs` | **Hints:** Add keybinding (e.g., `Ctrl+T` cycles themes, or `:theme <name>` via command palette). At minimum, use `get_theme_list()` to discover available themes, cycle through them on keypress. Reload the `Theme` struct and trigger a full redraw. Since `App` owns `Theme`, replacement is straightforward: `self.theme = Theme::load(theme_name)?`. | **Effort:** S

### 3.2 Archive Preview

- [ ] **3.2.1 Add `tar` and `zip` crates**
  - **P1** | **Files:** `Cargo.toml` | **Hints:** `tar = "0.4"`, `zip = "2"`. For `.tar.gz` use `flate2 = "1"`. | **Effort:** S

- [ ] **3.2.2 Implement archive contents preview in preview pane**
  - **P1** | **Files:** `src/ui/popup_preview.rs` | **Hints:** When file type is Archive (detected by `file` command or extension), list archive contents instead of raw bytes. For `.zip`: iterate entries, show name + size + compressed size. For `.tar`/`.tar.gz`: iterate entries, show name + size + mode. Format as a table similar to the file listing. Limit to first 1000 entries to avoid hanging. | **Effort:** M

- [ ] **3.2.3 Implement archive browsing as virtual directories (enter to descend into archive)**
  - **P1** | **Files:** `src/ui/panes.rs`, `src/fs/archive.rs` (new) | **Hints:** When entering a `.zip`/`.tar`/`.tar.gz` file, treat it as a directory. Show archive contents in the pane. Allow navigating up via `..` (back to real directory). Show archive path in pane title: `/path/to/archive.zip::`. Read full archive into memory for small archives; stream for large ones. | **Effort:** L

### 3.3 PDF/Document Preview

- [ ] **3.3.1 Add PDF text extraction**
  - **P2** | **Files:** `Cargo.toml`, `src/ui/popup_preview.rs` | **Hints:** Use `pdf_extract = "0.7"` or `lopdf` for PDF text extraction. For `.docx`, use `docx-rs`. Extract text content and display in preview pane. Fall back to "Binary file — cannot preview" if extraction fails. | **Effort:** M

- [ ] **3.3.2 Improve unknown file type handling**
  - **P1** | **Files:** `src/ui/popup_preview.rs:98-101` | **Hints:** Show more useful info for unknown/binary files: file size, MIME type, hex dump (first 256 bytes), file permissions, owner, modification time. Use `file` command output when available. | **Effort:** S

### 3.4 Bookmarks & History

- [ ] **3.4.1 Implement bookmark storage (TOML file)**
  - **P1** | **Files:** `src/bookmarks.rs` (new), `Cargo.toml` | **Hints:** Store bookmarks in `~/.config/rodeo/bookmarks.toml`. Structure: `[[bookmarks]]\npath = "/home/user/projects"\nname = "Projects"\npinned = false`. Add `toml = "0.8"` and `serde` for serialization. | **Effort:** M

- [ ] **3.4.2 Add bookmark keybindings: `m` to mark, `` ` `` to jump**
  - **P1** | **Files:** `src/ui/input.rs`, `src/ui/popup_bookmarks.rs` (new) | **Hints:** `m` bookmarks the current directory. `` ` `` opens a bookmark list popup. Navigate with `j`/`k`, `Enter` to jump, `d` to delete bookmark. Show bookmark name as label in popup. | **Effort:** M

- [ ] **3.4.3 Implement directory history (back/forward navigation)**
  - **P1** | **Files:** `src/ui/panes.rs`, `src/ui/input.rs` | **Hints:** Store visited directories in a `Vec<String>` per pane. Bindings: `Ctrl+o` = go back, `Ctrl+i` = go forward (or `H`/`L` in Vim style). Max history: 100 entries. Persist across sessions (optional). | **Effort:** M

- [ ] **3.4.4 Add recent-files tracking**
  - **P2** | **Files:** `src/bookmarks.rs` | **Hints:** Automatically track recently opened/edited files. Show in a "Recent" section of the bookmark popup. Max 50 entries. | **Effort:** S

### 3.5 File Watching

- [ ] **3.5.1 Add `notify` crate for live directory refresh**
  - **P1** | **Files:** `Cargo.toml` | **Hints:** `notify = { version = "7", features = ["macos_kqueue"] }`. Cross-platform filesystem events. | **Effort:** S

- [ ] **3.5.2 Implement auto-refresh on external changes**
  - **P1** | **Files:** `src/ui/mod.rs`, `src/ui/panes.rs` | **Hints:** Spawn a `notify` watcher on the current pane's directory. On `EventKind::Create | Modify | Remove`, reload the pane. Debounce: wait 100ms after last event before reloading (filesystem events come in bursts). Use `tokio` or a separate thread with `std::sync::mpsc` channel. IMPORTANT: The `notify` watcher and the TUI render loop run in different threads. In the main event loop, use `try_recv()` (non-blocking) on the channel to check for filesystem events without blocking keyboard input. If using tokio, integrate with `tokio::select!` alongside crossterm's `event::poll`. Debounce events: collect all events within a 100ms window before triggering a single pane reload. | **Effort:** M

---

## Phase 4: Power User (M5)

> Goal: Bulk rename, trash, git column, shell integration, directory sizes.
> Depends on: Phase 1 (operations), Phase 3 (bookmarks/history).

- [ ] **4.1 Bulk rename with regex and sequential patterns**
  - **P1** | **Files:** `src/ui/bulk_rename.rs` (new), `src/ui/input.rs` | **Hints:** Enter visual mode (`v`), select files, press `:b` or `Ctrl+R`. Show a two-column preview: old names → new names. Input bar at bottom: `s/old/new/` for regex substitution, `%d` for zero-padded numbering. Live preview of rename results. Confirm to apply. Use `regex` crate. Handle collisions before applying. | **Effort:** L

- [ ] **4.2 Trash support with restore capability**
  - **P1** | **Files:** `src/fs/ops.rs`, `src/ui/input.rs` | **Hints:** Use `trash` crate's `delete` and `list` APIs. Add a "Trash" view accessible via `:trash` command or similar. Show trash contents in a pane. Allow restore (`p` from trash) and permanent delete (`Shift+dd` from trash). Cross-platform: freedesktop Trash spec on Linux, macOS Trash, Windows Recycle Bin. | **Effort:** L

- [ ] **4.3 Git status column in file listing**
  - **P2** | **Files:** `src/ui/panes.rs` | **Hints:** Instead of just showing selected marker (`●`) in column 0, show git status: `M` modified, `A` added, `D` deleted, `?` untracked, `!` ignored, ` ` clean. Use `gix` crate (pure Rust) for git status parsing: `gix = "0.70"`. Cache git status per directory to avoid repeated `git status` calls. Update only when directory changes or on manual refresh. | **Effort:** L

- [ ] **4.4 Directory size calculation**
  - **P2** | **Files:** `src/fs/size.rs` (new), `src/ui/panes.rs` | **Hints:** For directories, show cumulative size instead of "DIR". Compute with parallel walk using `ignore` crate. Cache results. Show in the Size column: `12.3 MB` for dirs instead of `DIR`. Add a "calculating..." placeholder while scanning. `Ctrl+Shift+S` to trigger manual scan of current directory. | **Effort:** M

- [ ] **4.5 Shell command output in preview pane**
  - **P2** | **Files:** `src/ui/popup_cmd.rs`, `src/ui/popup_preview.rs` | **Hints:** `:!<cmd>` captures stdout/stderr and shows it in the preview pane (reusing preview infrastructure). Allow piping selected files to commands: `:!wc -l %f`. | **Effort:** M

- [ ] **4.6 External editor integration polish**
  - **P2** | **Files:** `src/ui/input.rs` | **Hints:** Support `$VISUAL` before `$EDITOR`. Add configuration option for default editor. When editor exits, reload the file's directory. If file was modified (check mtime), show a brief notification. | **Effort:** S

- [ ] **4.7 Config hot-reloading**
  - **P2** | **Files:** `src/config.rs`, `src/ui/input.rs` | **Hints:** Add `:so[urce]` command to reload config file at runtime. Watch config file with `notify` for auto-reload. | **Effort:** M

- [ ] **4.8 Multiple file selection with wildcards**
  - **P2** | **Files:** `src/ui/input.rs`, `src/ui/panes.rs` | **Hints:** `*` key to select all files matching a glob pattern (input dialog). `Ctrl+A` to select all files in current pane. `Esc` to clear selection. | **Effort:** S

- [ ] **4.9 Implement configurable keybindings from config file**
  - **P2** | **Files:** `src/config.rs`, `src/ui/input.rs` | **Hints:** Add `[keybindings]` section to config with string-to-action mappings. Define an `Action` enum representing every possible user action. In `input.rs`, instead of matching on `KeyCode` directly, build a `HashMap<KeyEvent, Action>` from config (with hardcoded defaults as fallback). This enables users to remap any key without code changes. Start simple: only support single-key bindings initially, expand to key sequences later. | **Effort:** M

---

## Phase 5: Infrastructure

> Goal: Tests, CI/CD, proper error handling, logging, documentation.
> Depends on: None. Should run in parallel with all phases.

### 5.1 Testing

- [ ] **5.1.1 Add unit tests for `format_size()`**
  - **P0** | **Files:** `src/ui/panes.rs` (add `#[cfg(test)] mod tests`) | **Hints:** Test cases: 0 → "0 B", 1023 → "1023 B", 1024 → "1.0 KB", 1048576 → "1.0 MB", 1073741824 → "1.0 GB". | **Effort:** S

- [ ] **5.1.2 Add unit tests for `format_date()`**
  - **P0** | **Files:** `src/ui/panes.rs` | **Hints:** Test with known `SystemTime` values. | **Effort:** S

- [ ] **5.1.3 Add unit tests for `Entry::new()`**
  - **P0** | **Files:** `src/ui/panes.rs` | **Hints:** Test with a temp file: create tempfile, construct Entry, verify kind=File, name matches, size > 0. Test with temp dir: kind=Directory. Test with nonexistent path: kind=Unknown. | **Effort:** S

- [ ] **5.1.4 Add unit tests for `Pane::next_index()`**
  - **P0** | **Files:** `src/ui/panes.rs` | **Hints:** Test wrap-around (last+down=0, first+up=last), single-item list, empty list (row_count=0), None selected → 0. | **Effort:** S

- [ ] **5.1.5 Add unit tests for sort logic in `read_entries()`**
  - **P1** | **Files:** `src/ui/panes.rs` | **Hints:** Extract sort to a pure function `sort_entries(entries, config) -> Vec<Entry>`. Test each SortType combination with SortOrder. Test directories_on_top ordering. | **Effort:** M

- [ ] **5.1.6 Add unit tests for `Config` deserialization**
  - **P1** | **Files:** `src/config.rs` | **Hints:** Test default values, test YAML parsing with partial config, test missing fields → defaults. | **Effort:** S

- [ ] **5.1.7 Add unit tests for `Theme` deserialization**
  - **P1** | **Files:** `src/ui/theme.rs` | **Hints:** Test hex color parsing (valid, invalid, short strings). Test loading a known theme file. | **Effort:** S

- [ ] **5.1.8 Add integration test: app starts and renders without panic**
  - **P1** | **Files:** `tests/integration.rs` (new) | **Hints:** Use `ratatui::Terminal::new(CrosstermBackend::new(io::sink()))` or `ratatui::backend::TestBackend`. Run one frame of `App::render()`. Assert no panic. | **Effort:** M

- [ ] **5.1.9 Add integration tests for file operations (requires temp dirs)**
  - **P2** | **Files:** `tests/file_ops.rs` (new) | **Hints:** Create temp directory with test files. Run copy/move/delete/rename. Assert filesystem state. Use `tempfile` crate. | **Effort:** L

- [ ] **5.1.10 Add unit tests for dialog module (once created)**
  - **P1** | **Files:** `src/ui/dialog.rs` | **Hints:** Test confirm dialog returns correct result on y/n/Esc/Enter. Test input dialog buffers text correctly. | **Effort:** S

### 5.2 CI/CD

- [ ] **5.2.1 Create GitHub Actions workflow: test, clippy, fmt**
  - **P1** | **Files:** `.github/workflows/ci.yml` (new) | **Hints:** Matrix: stable + beta Rust. Steps: checkout, install Rust, cache cargo, `cargo test --all`, `cargo clippy -- -D warnings`, `cargo fmt --check`. Add `cargo check` for quick validation. | **Effort:** S

- [ ] **5.2.2 Add `cargo-deny` to CI for license/security auditing**
  - **P2** | **Files:** `.github/workflows/ci.yml`, `deny.toml` (new) | **Hints:** `cargo install cargo-deny && cargo deny check`. Configure `deny.toml` to allow common licenses (MIT, Apache-2.0, BSD, etc). | **Effort:** S

- [ ] **5.2.3 Add code coverage reporting (tarpaulin or llvm-cov)**
  - **P2** | **Files:** `.github/workflows/ci.yml` | **Hints:** `cargo install cargo-tarpaulin && cargo tarpaulin --out Lcov`. Upload to coveralls or codecov. | **Effort:** M

- [ ] **5.2.4 Add release build workflow**
  - **P2** | **Files:** `.github/workflows/release.yml` (new) | **Hints:** Trigger on tag push. Build with `--release`. Upload binary as release artifact. Consider `cargo-dist` or manual matrix for linux-x64, macos-arm64, macos-x64. | **Effort:** M

### 5.3 Error Handling

- [ ] **5.3.1 Replace `env_logger` + `log` with `tracing` + `color-eyre`**
  - **P1** | **Files:** `Cargo.toml`, `src/main.rs`, all `log::` imports | **Hints:** Planning docs recommend `tracing` for structured logging and `color-eyre` for error reporting. `color-eyre` is already in Cargo.toml. Replace `log::info!` → `tracing::info!`, etc. Use `tracing-subscriber` for output. Add file-based logging with `tracing-appender`. | **Effort:** M

- [ ] **5.3.2 Convert `main() -> io::Result<()>` to use `color_eyre::Result<()>`**
  - **P1** | **Files:** `src/main.rs` | **Hints:** `color_eyre::install()?;` at top. Change return type. This gives colorful, detailed error traces for all `?` propagations. | **Effort:** S

- [ ] **5.3.3 Systematic `.unwrap()` / `.expect()` removal**
  - **P1** | **Files:** All `src/**/*.rs` | **Hints:** Convert panicking functions to return `Result`. Use `?` propagation. For truly unrecoverable errors (e.g., terminal init failure), use `.expect()` with a descriptive message. File listing errors should not crash — show error in pane body. | **Effort:** L

- [ ] **5.3.4 Add error display in footer/pane for non-fatal errors**
  - **P2** | **Files:** `src/ui/footer.rs`, `src/ui/mod.rs` | **Hints:** Add `error_message: Option<String>` to `UiConfig` or `Footer`. On file operation failure, set the message and show it in the footer (or as a temporary status bar notification). Auto-clear after 3 seconds. | **Effort:** M

### 5.4 Documentation

- [ ] **5.4.1 Add crate-level doc comments (`//!`) to `src/lib.rs` or `src/main.rs`**
  - **P2** | **Files:** `src/main.rs`, `src/lib.rs` (new, optional) | **Hints:** Describe what rodeo is, link to README, document module structure. | **Effort:** S

- [ ] **5.4.2 Add doc comments to all public items**
  - **P2** | **Files:** All `src/**/*.rs` | **Hints:** Every `pub struct`, `pub fn`, `pub enum` should have `///` doc comments. Run `cargo doc --open` to verify. Enable `#![warn(missing_docs)]` once most items are documented. | **Effort:** M

- [ ] **5.4.3 Update README with current status, installation instructions, keybindings**
  - **P2** | **Files:** `README.md` | **Hints:** Add "Installation" section (`cargo install --path .` or `cargo build --release`). Add runtime deps section: `bat`, `file` commands (until syntect replaces bat). Add keybinding table. Link to this TODO. | **Effort:** M

- [ ] **5.4.4 Add man page or `--help` improvement**
  - **P3** | **Files:** `src/cli.rs`, `docs/rodeo.1` (new) | **Hints:** Enhance clap doc strings. Optional: generate man page with `clap_mangen`. | **Effort:** S

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
- Phase 1.5 (async) requires tokio and `src/fs/ops.rs` extraction from Phase 1.2.
- Phase 3 syntect replacement has no blockers — can start any time.
- Phase 4 bulk rename needs visual mode (1.3) and file ops (1.2).
- Phase 4 git column has no blockers (git info already in header).
- Phase 5 runs in parallel with everything.

---

## Crate Dependencies

### Current (in `Cargo.toml`)

| Crate | Version | Used? | Notes |
|-------|---------|-------|-------|
| `ansi-to-tui` | 8.0.1 | Yes | Converts bat ANSI output to ratatui Text |
| `chrono` | 0.4.45 | Yes | File modification time formatting |
| `clap` | 4.6.1 | Yes | CLI argument parsing |
| `color-eyre` | 0.6.5 | **No** | Declared but never imported — dead dep |
| `crossterm` | 0.29.0 | Yes | Terminal input events |
| `env_logger` | 0.11.10 | Yes | Logging (to be replaced by `tracing`) |
| `image` | 0.25.10 | Yes | Image preview decoding |
| `log` | 0.4.31 | Yes | Log macros (to be replaced by `tracing`) |
| `ratatui` | 0.30.0 | Yes | Core TUI framework |
| `ratatui-image` | 11.0.4 | Yes | Terminal image rendering |
| `serde` | 1.0.228 | Yes | Config/theme deserialization |
| `xdg` | 3.0.0 | Yes | XDG base directories for config path |
| `yaml_serde` | 0.10.4 | Yes | YAML config parsing (see Open Decisions) |

### New Crates Needed (by priority)

| Crate | Version | Phase | Purpose |
|-------|---------|-------|---------|
| `trash` | 5 | Phase 1 | Safe delete to system trash |
| `tokio` | 1 | Phase 1 | Async file operations + file watching |
| `regex` | 1 | Phase 2 | Regex filter + find-in-files |
| `nucleo` | 0.5 | Phase 2 | Fuzzy file matching |
| `ignore` | 0.4 | Phase 2 | File tree walking (respects .gitignore) |
| `syntect` | 5 | Phase 3 | Syntax highlighting (replace `bat`) |
| `toml` | 0.8 | Phase 3 | Bookmark storage (and possibly config) |
| `tar` | 0.4 | Phase 3 | Tar archive reading |
| `zip` | 2 | Phase 3 | Zip archive reading |
| `flate2` | 1 | Phase 3 | Gzip decompression for `.tar.gz` |
| `notify` | 7 | Phase 3 | Filesystem event watching |
| `tracing` | 0.1 | Phase 5 | Structured logging (replace `log`) |
| `tracing-subscriber` | 0.3 | Phase 5 | Log output formatting |
| `tracing-appender` | 0.2 | Phase 5 | File-based log rotation (optional) |
| `gix` | 0.70 | Phase 4 | Pure-Rust git status (replace `git` CLI) |
| `pdf-extract` | 0.7 | Phase 3 | PDF text extraction (optional) |
| `tempfile` | 3 | Phase 5 | Test utilities |

> **Note:** Crate versions above are current as of 2026-06-19. Check [crates.io](https://crates.io) for latest versions before adding. Use `cargo add <crate>` for automatic version resolution.

### Crates to Remove

| Crate | Reason |
|-------|--------|
| `yaml_serde` | If switching config to TOML (use `toml` crate instead) |
| `env_logger` | If switching to `tracing-subscriber` |
| `log` | If switching to `tracing` (use `tracing` macros) |
| `color-eyre` | If not used (otherwise integrate properly) |
| `ansi-to-tui` | If replacing `bat` with `syntect` (syntect produces spans directly) |

---

## Open Decisions

| # | Question | Options | Impact | Recommendation |
|---|----------|---------|--------|----------------|
| 1 | **YAML vs TOML for config?** | YAML (current) or TOML (planning docs) | Affects `config.rs`, theme files, Cargo.toml deps, user-facing format | **TOML** — more Rust-idiomatic, serde support is first-class, no indentation issues, README and all planning docs assume TOML. However, 10 theme files already exist in YAML — migration script needed. |
| 2 | **When to introduce async (tokio)?** | Phase 1.5 (M2) or Phase 3 (M4) | Affects architecture and error handling complexity | **Phase 1.5** — async progress bars for copy/move are a key UX differentiator. But keep core navigation synchronous. Use `tokio::task::spawn_blocking` for heavy FS work. |
| 3 | **Vim-modal or modeless?** | Modal (Normal/Command/Visual) or single-mode with key combos | Fundamental UX design. Affects all keybinding code. | **Modal** — all three planning docs agree. Vim users are the target audience. Start modeless in Phase 1.2, introduce modes in 1.3. |
| 4 | **`syntect` vs keep `bat`?** | `syntect` (pure Rust, bundled) vs `bat` (external binary) | Preview architecture, binary size, runtime deps | **`syntect`** — all planning docs recommend it. Removes external dependency, works offline, gives full control over theme mapping. `bat` was a quick prototype shortcut. |
| 5 | **`gix` vs `git` CLI?** | `gix` (pure Rust) vs shelling out to `git` binary | Git status performance, binary size, portability | **`gix`** — removes runtime dependency on `git` binary, no shell overhead, consistent behavior. But lower priority: current `git` CLI approach works fine for header stats. |
| 6 | **Single crate or workspace?** | Single crate vs `rodeo-core` + `rodeo-tui` | Build complexity, compile times, API boundaries | **Single crate** for now — project is small (~1600 lines). Split when it hurts: when adding SFTP, or when a library API is needed. All planning docs agree. |
| 7 | **Keybinding customization?** | Hardcoded vs configurable in `config.toml` | User experience, code complexity | **Configurable** (eventually) — start hardcoded, but design the input system to accept a keybinding map. Add to config after Phase 1.3 when the keybinding surface stabilizes. |
| 8 | **Linux-first or cross-platform from day 1?** | Linux only vs Linux + macOS + Windows | Dependency choices, testing burden | **Linux-first** with cross-platform awareness. Use cross-platform crates (`crossterm`, `notify`, `trash`). Don't test on macOS/Windows initially, but avoid platform-specific code. |
| 9 | **Trash: permanent delete fallback?** | Only trash (fail on unsupported FS) vs fallback to `rm` | Safety vs. functionality | **Trash with permanent fallback** — if `trash` crate fails (network FS, some Linux configs), show a prominent "Trash unavailable. Permanently delete?" confirmation with red styling. |
| 10 | **Single binary or multiple?** | One `rodeo` binary vs `rodeo` + `rodeo-server` for remote | Distribution simplicity vs. capability | **Single binary** — remote filesystem features are Phase 6+. No server mode needed now. |

---

## Summary: Effort Estimates by Phase

| Phase | Tasks | Total Estimate | Can Parallelize? |
|-------|-------|---------------|-------------------|
| Quick Wins | 15 | ~2 hours | All independent |
| Phase 0: Bug Fixes | 21 | ~3 days | Most independent |
| Phase 1: File Ops | 18 | ~2-3 weeks | 1.1-1.2 first, then 1.3-1.5 |
| Phase 2: Search | 7 | ~1-2 weeks | Can start after 0 |
| Phase 3: Preview | 11 | ~2 weeks | Can start after 0 |
| Phase 4: Power User | 9 | ~3 weeks | Needs Phase 1+2+3 partially |
| Phase 5: Infrastructure | 18 | ~2 weeks | Runs in parallel |
| **Total** | **99** | **~10-12 weeks part-time** | |

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
