mod app;
mod markdown;

use std::{
    collections::HashMap,
    env, fs,
    io::{self, stdout, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use app::App;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use markdown::{heading_block_colors, markdown_to_render};
use ratatui::{
    backend::CrosstermBackend,
    style::{Color, Modifier, Style},
    Terminal,
};

fn main() -> io::Result<()> {
    let args = parse_args().unwrap_or_else(|| {
        eprintln!("Usage: md-viewer [--dump] [--help] <path-to-markdown>");
        std::process::exit(2);
    });

    if args.dump {
        dump_file(&args.path)?;
        return Ok(());
    }

    let mut app = App::load(&args.path)?;

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

fn parse_args() -> Option<Args> {
    let mut dump = false;
    let mut path = None;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                return None;
            }
            "--dump" => dump = true,
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => {
                path = Some(PathBuf::from(arg));
                break;
            }
        }
    }
    path.map(|path| Args { path, dump })
}

fn print_help() {
    println!("md-viewer");
    println!("Usage: md-viewer [--dump] <path-to-markdown>\n");
    println!("Options:");
    println!("  --dump       Render the file as ANSI text instead of launching the TUI");
    println!("  --help, -h   Show this help text");
}

struct Args {
    path: PathBuf,
    dump: bool,
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

fn dump_file(path: &Path) -> io::Result<()> {
    let markdown = fs::read_to_string(path)?;
    let render = markdown_to_render(&markdown);
    let mut heading_bg = HashMap::new();
    for heading in &render.headings {
        let (bg, _) = heading_block_colors(heading.level);
        heading_bg.insert(heading.line, bg);
    }
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let mut out = io::BufWriter::new(io::stdout());
    for (idx, line) in render.lines.iter().enumerate() {
        let base_bg = heading_bg.get(&idx).copied();
        let mut rendered_width = 0usize;
        for span in &line.spans {
            write!(
                out,
                "{}{}{}",
                style_prefix(span.style, base_bg),
                span.content,
                ANSI_RESET
            )?;
            rendered_width += span.content.chars().count();
        }
        if let Some(bg) = base_bg {
            if rendered_width < term_width {
                let remaining = term_width - rendered_width;
                let filler_style = Style::default().bg(bg);
                write!(
                    out,
                    "{}{}{}",
                    style_prefix(filler_style, Some(bg)),
                    " ".repeat(remaining),
                    ANSI_RESET
                )?;
            }
        }
        writeln!(out)?;
    }
    out.flush()
}

const ANSI_RESET: &str = "\x1b[0m";

fn style_prefix(mut style: Style, default_bg: Option<Color>) -> String {
    if style.bg.is_none() {
        style.bg = default_bg;
    }
    let mut codes: Vec<String> = Vec::new();
    if let Some(fg) = style.fg {
        codes.push(color_code(fg, true));
    }
    if let Some(bg) = style.bg {
        codes.push(color_code(bg, false));
    }
    let modifiers = style.add_modifier;
    if modifiers.contains(Modifier::BOLD) {
        codes.push("1".into());
    }
    if modifiers.contains(Modifier::DIM) {
        codes.push("2".into());
    }
    if modifiers.contains(Modifier::ITALIC) {
        codes.push("3".into());
    }
    if modifiers.contains(Modifier::UNDERLINED) {
        codes.push("4".into());
    }
    if modifiers.contains(Modifier::REVERSED) {
        codes.push("7".into());
    }
    if modifiers.contains(Modifier::CROSSED_OUT) {
        codes.push("9".into());
    }
    if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}

fn color_code(color: Color, is_fg: bool) -> String {
    match color {
        Color::Reset => (if is_fg { "39" } else { "49" }).into(),
        Color::Black => ansi_basic(30, 40, is_fg),
        Color::Red => ansi_basic(31, 41, is_fg),
        Color::Green => ansi_basic(32, 42, is_fg),
        Color::Yellow => ansi_basic(33, 43, is_fg),
        Color::Blue => ansi_basic(34, 44, is_fg),
        Color::Magenta => ansi_basic(35, 45, is_fg),
        Color::Cyan => ansi_basic(36, 46, is_fg),
        Color::Gray => ansi_basic(37, 47, is_fg),
        Color::DarkGray => ansi_basic(90, 100, is_fg),
        Color::LightRed => ansi_basic(91, 101, is_fg),
        Color::LightGreen => ansi_basic(92, 102, is_fg),
        Color::LightYellow => ansi_basic(93, 103, is_fg),
        Color::LightBlue => ansi_basic(94, 104, is_fg),
        Color::LightMagenta => ansi_basic(95, 105, is_fg),
        Color::LightCyan => ansi_basic(96, 106, is_fg),
        Color::White => ansi_basic(97, 107, is_fg),
        Color::Indexed(idx) => {
            let base = if is_fg { 38 } else { 48 };
            format!("{};5;{}", base, idx)
        }
        Color::Rgb(r, g, b) => {
            let base = if is_fg { 38 } else { 48 };
            format!("{};2;{};{};{}", base, r, g, b)
        }
    }
}

fn ansi_basic(fg: u8, bg: u8, is_fg: bool) -> String {
    if is_fg {
        fg.to_string()
    } else {
        bg.to_string()
    }
}
