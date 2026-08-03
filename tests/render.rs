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
    for key in [' ', 'j', ' ', '?', '?', 'a'] {
        app.dispatch_key(&crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(key),
            crossterm::event::KeyModifiers::NONE,
        ));
        draw(&mut app, 120, 30);
    }

    app.dispatch_key(&crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    draw(&mut app, 120, 30);
}

/// Find-in-files shows the hit list and, next to it, the file around the
/// matching line — the whole point of the split, so it is asserted on.
#[test]
fn find_in_files_previews_the_selected_hit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.rs"),
        "fn one() {}\nfn needle() {}\nfn three() {}\n",
    )
    .unwrap();
    let mut app = app_in(dir.path());

    app.dispatch_key(&crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('g'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for c in "needle".chars() {
        app.dispatch_key(&crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.dispatch_key(&crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    let buffer = draw(&mut app, 140, 40);
    let text: String = buffer.content().iter().map(|c| c.symbol()).collect();
    // The hit, listed relative to the search root...
    assert!(text.contains("a.rs:2"), "{text}");
    // ...and its neighbouring lines, which only the preview pane can show.
    assert!(text.contains("fn three() {}"), "{text}");

    // Too narrow for two columns: the list keeps the full width and nothing
    // panics.
    draw(&mut app, 60, 20);
}

/// The About popup was folded into the help popup, so the version has to be
/// visible there — it is now the only place that shows it in the app.
#[test]
fn the_help_popup_shows_the_version() {
    let dir = populated_dir();
    let mut app = app_in(dir.path());

    app.dispatch_key(&crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('?'),
        crossterm::event::KeyModifiers::NONE,
    ));

    let buffer = draw(&mut app, 140, 40);
    let text: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(
        text.contains(&format!("rodeo v{}", env!("CARGO_PKG_VERSION"))),
        "{text}"
    );
}
