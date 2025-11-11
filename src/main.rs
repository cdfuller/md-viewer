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
    if app.is_help_open() {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
            _ => {}
        }
        return Ok(false);
    }
    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Up | KeyCode::Char('k') => app.scroll_up(1),
        KeyCode::Down | KeyCode::Char('j') => app.scroll_down(1),
        KeyCode::PageUp | KeyCode::Char('p') => app.page_up(),
        KeyCode::PageDown | KeyCode::Char('n') => app.page_down(),
        KeyCode::Char(' ') => app.page_down(),
        KeyCode::Home | KeyCode::Char('g') => app.scroll_to(0),
        KeyCode::End | KeyCode::Char('G') => app.scroll_to_end(),
        KeyCode::Char('r') => match app.reload() {
            Ok(()) => app.set_status("Reloaded file"),
            Err(err) => app.set_status(format!("Reload failed: {err}")),
        },
        KeyCode::Char('?') => app.toggle_help(),
        _ => {}
    }

    Ok(false)
}
