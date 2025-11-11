mod app;
mod markdown;

use std::{
    env,
    io::{self, stdout},
    path::PathBuf,
    time::Duration,
};

use app::App;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

fn main() -> io::Result<()> {
    let Some(path) = env::args().nth(1).map(PathBuf::from) else {
        eprintln!("Usage: md-viewer <path-to-markdown>");
        std::process::exit(2);
    };

    let mut app = App::load(&path)?;

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| app.draw(frame))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if handle_key(app, key)? {
                    break;
                }
            }
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('p') => app.scroll_up(1),
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('n') => app.scroll_down(1),
        KeyCode::PageUp => app.scroll_up(app.viewport_height().saturating_sub(1)),
        KeyCode::PageDown => app.scroll_down(app.viewport_height().saturating_sub(1)),
        KeyCode::Char(' ') if key.modifiers.is_empty() => {
            app.scroll_down(app.viewport_height().saturating_sub(1))
        }
        KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.scroll_up(app.viewport_height().saturating_sub(1))
        }
        KeyCode::Home | KeyCode::Char('g') => app.scroll_to(0),
        KeyCode::End | KeyCode::Char('G') => app.scroll_to_end(),
        KeyCode::Char('r') => match app.reload() {
            Ok(()) => app.set_status("Reloaded file"),
            Err(err) => app.set_status(format!("Reload failed: {err}")),
        },
        _ => {}
    }

    Ok(false)
}
