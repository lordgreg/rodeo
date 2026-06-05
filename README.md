# rodeo 🤠

Rodeo is a modern terminal file manager inspired by Norton and Midnight Commander, built in Rust. It combines the classic dual-pane interface with Vim- keybindings and a focus on speed and usability. The goal is to create a tool that feels good to use daily, with a clean codebase and a clear roadmap for future features.

# Roadmap

## M0: Foundation

- [x] Project scaffolding with Cargo workspaces
- [x] Input handling with Clap
- [x] Load config from `~/.config/rodeo/config.toml` using Serde and Derive
- [x] Basic terminal UI setup with Ratatui

## M1: Navigation

- ~~[ ] Single-pane file browser~~
- [x] `hjkl` navigation
- [x] `Enter` to open directories
- [x] `q` to quit

## M2: Operations

- [ ] File operations: copy, move, delete, rename, mkdir
- [ ] Confirmation dialogs for destructive actions
- [ ] Async file operations with progress bar
- [ ] Command palette (`:`) for quick actions

M3: Search & Filter

- [ ] Fuzzy file search (`/` to activate)
- [ ] Regex filter (`Ctrl+F` to activate)
- [ ] Find-in-files (search file contents)

M4: Preview & Polish

- [ ] Preview panel with text, syntax-highlighted code, image thumbnails
- [x] Theming

M5: Power User

- [ ] Git integration (status icons, branch indicator)
- [ ] Bookmarks & history
- [ ] Bulk rename with regex and sequential patterns
- [ ] Archive support (browse `zip`, `tar`, `tar.gz` as virtual directories)
- [ ] Trash support (safe delete instead of permanent removal)
