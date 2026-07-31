//! Rendering smoke tests.
//!
//! The TUI is otherwise only exercised by hand: these drive `App::render()`
//! against a headless `TestBackend`, so a panic in any layout or widget path
//! fails the build instead of the next manual run.

use ratatui::{Terminal, backend::TestBackend};
use rodeo::config::Config;
use rodeo::ui::App;
use rodeo::ui::theme::Theme;

fn app_in(dir: &std::path::Path) -> App {
    let config = Config {
        initial_directory_left: dir.to_string_lossy().to_string(),
        initial_directory_right: dir.to_string_lossy().to_string(),
        ..Default::default()
    };
    App::new(Theme::builtin().expect("built-in theme"), config)
}

fn draw(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("render must not fail");
    terminal.backend().buffer().clone()
}

/// A directory with one of everything the panes have to format.
fn populated_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn main() {}").unwrap();
    std::fs::write(dir.path().join("b.md"), "# title").unwrap();
    std::fs::write(dir.path().join(".hidden"), "x").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("a.rs"), dir.path().join("link")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("gone"), dir.path().join("broken")).unwrap();
    dir
}

#[test]
fn renders_without_panic() {
    let dir = populated_dir();
    let mut app = app_in(dir.path());

    let buffer = draw(&mut app, 120, 30);
    assert!(
        buffer.content().iter().any(|cell| cell.symbol() == "a"),
        "the listing should have been drawn"
    );
}

#[test]
fn renders_at_many_terminal_sizes() {
    let dir = populated_dir();
    let mut app = app_in(dir.path());

    // Includes sizes small enough that every column has to be dropped, and
    // the degenerate 1x1 case.
    for (width, height) in [
        (1, 1),
        (10, 3),
        (40, 10),
        (80, 24),
        (120, 30),
        (200, 50),
        (400, 100),
    ] {
        draw(&mut app, width, height);
    }
}

#[test]
fn renders_an_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());

    let buffer = draw(&mut app, 80, 24);
    let text: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(text.contains("(empty directory)"), "{text}");
}

#[test]
fn renders_with_icons_enabled() {
    let dir = populated_dir();
    let config = Config {
        initial_directory_left: dir.path().to_string_lossy().to_string(),
        initial_directory_right: dir.path().to_string_lossy().to_string(),
        icons: true,
        show_hidden: true,
        ..Default::default()
    };
    let mut app = App::new(Theme::builtin().unwrap(), config);

    draw(&mut app, 120, 30);
}

#[test]
fn renders_every_overlay() {
    let dir = populated_dir();
    let mut app = app_in(dir.path());

    // Each overlay dims the frame beneath it and draws its own geometry;
    // rendering them one after another covers those paths.
    for key in ['?', ' ', 'j', ' '] {
        app.dispatch_key(&crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(key),
            crossterm::event::KeyModifiers::NONE,
        ));
        draw(&mut app, 120, 30);
    }

    for code in [
        crossterm::event::KeyCode::F(1),
        crossterm::event::KeyCode::F(7),
        crossterm::event::KeyCode::Esc,
    ] {
        app.dispatch_key(&crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        ));
        draw(&mut app, 120, 30);
    }
}
