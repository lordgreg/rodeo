use ratatui::{DefaultTerminal, Frame};

mod panes;
fn app(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    loop {
        terminal.draw(render)?;
        if crossterm::event::read()?.is_key_press() {
            break Ok(());
        }
    }
}

fn render(frame: &mut Frame<'_>) {
    // frame.render_widget("hello world", frame.area());
    panes::ui_panes(frame);
}

pub fn run() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(app)?;
    Ok(())
}
