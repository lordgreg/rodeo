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

/// One frame, driven exactly as the run loop drives it: prepare, then paint.
///
/// The `prepare_frame` call is not incidental. The draw closure is paint-only
/// — it reads state and never builds it — so a test that skipped preparation
/// would render empty previews and prove nothing.
fn draw(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    app.prepare_frame();
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

/// `/` opens the file finder: names from subdirectories are found by typing,
/// the preview shows the selected file, and the border says what the walk is
/// not looking at.
#[test]
fn the_file_finder_searches_subdirectories_and_names_its_filter() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/deep")).unwrap();
    std::fs::write(dir.path().join("src/deep/needle.rs"), "fn found_me() {}\n").unwrap();
    std::fs::write(dir.path().join("other.txt"), "unrelated\n").unwrap();
    let mut app = app_in(dir.path());

    press(&mut app, '/', crossterm::event::KeyModifiers::NONE);
    for c in "needle".chars() {
        press(&mut app, c, crossterm::event::KeyModifiers::NONE);
    }

    let buffer = draw(&mut app, 140, 40);
    let text: String = buffer.content().iter().map(|c| c.symbol()).collect();
    // The match, with the path it was found under...
    assert!(text.contains("src/deep/needle.rs"), "{text}");
    // ...its contents in the preview pane...
    assert!(text.contains("fn found_me() {}"), "{text}");
    // ...and the filter in force, so a short list is never a mystery.
    assert!(text.contains("filter: gitignore"), "{text}");
    // Only the match is listed (the pane behind still shows the rest, so the
    // count in the title is what says the query narrowed things down).
    assert!(text.contains("Find Files — 1 of"), "{text}");

    // Narrow terminal: one column, no panic.
    draw(&mut app, 50, 20);
}

/// The one query box takes a regex just as happily as a word, and says which
/// of the two it is doing.
#[test]
fn the_file_finder_takes_a_regex_in_the_same_box() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("keep.rs"), "fn a() {}\n").unwrap();
    std::fs::write(dir.path().join("drop.txt"), "b\n").unwrap();
    let mut app = app_in(dir.path());

    press(&mut app, '/', crossterm::event::KeyModifiers::NONE);
    for c in r"\.rs$".chars() {
        press(&mut app, c, crossterm::event::KeyModifiers::NONE);
    }

    let buffer = draw(&mut app, 140, 40);
    let text: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(text.contains("keep.rs"), "{text}");
    // One of two candidates matched, and the box reports how it read the query.
    assert!(text.contains("Find Files — 1 of 2"), "{text}");
    assert!(text.contains("regex"), "{text}");
}

/// Enter on a hit puts the pane where the file lives, which is what a file
/// manager's finder is for.
#[test]
fn enter_in_the_file_finder_takes_the_pane_to_the_match() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/target.txt"), "x\n").unwrap();
    let mut app = app_in(dir.path());

    press(&mut app, '/', crossterm::event::KeyModifiers::NONE);
    for c in "target".chars() {
        press(&mut app, c, crossterm::event::KeyModifiers::NONE);
    }
    app.dispatch_key(&crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    let buffer = draw(&mut app, 140, 40);
    let text: String = buffer.content().iter().map(|c| c.symbol()).collect();
    // The pane now lists the containing directory, popup closed.
    assert!(text.contains("sub"), "{text}");
    assert!(text.contains("target.txt"), "{text}");
    assert!(!text.contains("Find Files"), "{text}");
}

fn press(app: &mut App, c: char, modifiers: crossterm::event::KeyModifiers) {
    app.dispatch_key(&crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(c),
        modifiers,
    ));
}
