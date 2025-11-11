mod app;
mod markdown;

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{self, stdout, Write},
    mem,
    path::{Path, PathBuf},
    time::Duration,
};

use app::App;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use markdown::{
    heading_block_colors, markdown_to_render, CodeBlockOverlay, CODE_BLOCK_BG, CODE_BLOCK_BORDER_FG,
};
use ratatui::{
    backend::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::Line,
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
    let rule_lines: HashSet<usize> = render.rules.iter().copied().collect();
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let mut out = io::BufWriter::new(io::stdout());
    let mut idx = 0usize;
    let mut code_iter = render.code_blocks.iter().peekable();
    while idx < render.lines.len() {
        if rule_lines.contains(&idx) {
            write_rule_line_dump(&mut out, term_width)?;
            idx += 1;
            continue;
        }
        if let Some(block) = code_iter.peek() {
            if idx == block.line_start {
                let end = block.line_end.min(render.lines.len());
                let slice = &render.lines[block.line_start..end];
                write_code_block_dump(&mut out, slice, block, term_width)?;
                idx = block.line_end;
                code_iter.next();
                continue;
            }
        }
        let line = &render.lines[idx];
        let base_bg = heading_bg.get(&idx).copied();
        write_regular_line_dump(&mut out, line, base_bg, term_width)?;
        idx += 1;
    }
    out.flush()
}

const ANSI_RESET: &str = "\x1b[0m";

fn write_regular_line_dump(
    out: &mut impl Write,
    line: &Line<'_>,
    base_bg: Option<Color>,
    term_width: usize,
) -> io::Result<()> {
    let rendered_width = write_line_content(out, line, base_bg)?;
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
    writeln!(out)
}

fn write_rule_line_dump(out: &mut impl Write, term_width: usize) -> io::Result<()> {
    let width = term_width.max(1);
    let style = Style::default().fg(Color::DarkGray);
    write!(
        out,
        "{}{}{}",
        style_prefix(style, None),
        "─".repeat(width),
        ANSI_RESET
    )?;
    writeln!(out)
}

fn write_code_block_dump(
    out: &mut impl Write,
    lines: &[Line<'_>],
    block: &CodeBlockOverlay,
    term_width: usize,
) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let available_width = term_width.saturating_sub(4).max(1);
    let mut rows: Vec<Vec<(Style, String)>> = Vec::new();
    for line in lines {
        let mut wrapped = wrap_line(line, available_width);
        rows.append(&mut wrapped);
    }
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    let content_width = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|(_, text)| text.chars().count())
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0)
        .max(1);
    let inner_width = content_width + 2;
    write_code_block_border(out, block.language.as_deref(), inner_width, true)?;
    let border_style = code_block_border_style();
    for row in &rows {
        write!(
            out,
            "{}│ {}",
            style_prefix(border_style, Some(CODE_BLOCK_BG)),
            ANSI_RESET
        )?;
        let rendered = write_segments(
            out,
            row.iter().map(|(style, text)| (*style, text.as_str())),
            Some(CODE_BLOCK_BG),
        )?;
        if rendered < content_width {
            let padding = content_width - rendered;
            let padding_style = Style::default().bg(CODE_BLOCK_BG);
            write!(
                out,
                "{}{}{}",
                style_prefix(padding_style, Some(CODE_BLOCK_BG)),
                " ".repeat(padding),
                ANSI_RESET
            )?;
        }
        write!(
            out,
            "{} │{}",
            style_prefix(border_style, Some(CODE_BLOCK_BG)),
            ANSI_RESET
        )?;
        writeln!(out)?;
    }
    write_code_block_border(out, None, inner_width, false)
}

fn write_code_block_border(
    out: &mut impl Write,
    title: Option<&str>,
    inner_width: usize,
    top: bool,
) -> io::Result<()> {
    let mut line = String::new();
    let (left, right) = if top { ('┌', '┐') } else { ('└', '┘') };
    line.push(left);
    if let Some(label) = title {
        let title_text = format!(" {} ", label);
        let title_len = title_text.chars().count();
        if title_len >= inner_width {
            let truncated: String = title_text.chars().take(inner_width).collect();
            line.push_str(&truncated);
        } else {
            line.push_str(&title_text);
            line.push_str(&"─".repeat(inner_width - title_len));
        }
    } else {
        line.push_str(&"─".repeat(inner_width));
    }
    line.push(right);
    write!(
        out,
        "{}{}{}",
        style_prefix(code_block_border_style(), Some(CODE_BLOCK_BG)),
        line,
        ANSI_RESET
    )?;
    writeln!(out)
}

fn write_line_content(
    out: &mut impl Write,
    line: &Line<'_>,
    default_bg: Option<Color>,
) -> io::Result<usize> {
    write_segments(
        out,
        line.spans
            .iter()
            .map(|span| (span.style, span.content.as_ref())),
        default_bg,
    )
}

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

fn code_block_border_style() -> Style {
    Style::default()
        .fg(CODE_BLOCK_BORDER_FG)
        .bg(CODE_BLOCK_BG)
        .add_modifier(Modifier::BOLD)
}

fn write_segments<'a, I>(
    out: &mut impl Write,
    segments: I,
    default_bg: Option<Color>,
) -> io::Result<usize>
where
    I: IntoIterator<Item = (Style, &'a str)>,
{
    let mut rendered_width = 0usize;
    for (style, text) in segments {
        write!(
            out,
            "{}{}{}",
            style_prefix(style, default_bg),
            text,
            ANSI_RESET
        )?;
        rendered_width += text.chars().count();
    }
    Ok(rendered_width)
}

fn wrap_line(line: &Line<'_>, width: usize) -> Vec<Vec<(Style, String)>> {
    if width == 0 {
        return vec![Vec::new()];
    }
    let mut rows: Vec<Vec<(Style, String)>> = Vec::new();
    let mut current: Vec<(Style, String)> = Vec::new();
    let mut current_width = 0usize;
    for span in &line.spans {
        let mut remaining = span.content.as_ref();
        while !remaining.is_empty() {
            let available = width - current_width;
            if available == 0 {
                rows.push(mem::take(&mut current));
                current_width = 0;
                continue;
            }
            let (prefix, rest, consumed) = take_prefix(remaining, available);
            if consumed == 0 {
                break;
            }
            current.push((span.style, prefix.to_string()));
            remaining = rest;
            current_width += consumed;
            if current_width == width {
                rows.push(mem::take(&mut current));
                current_width = 0;
            }
        }
    }
    rows.push(current);
    rows.retain(|row| !row.is_empty());
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
}

fn take_prefix(text: &str, limit: usize) -> (&str, &str, usize) {
    if limit == 0 {
        return ("", text, 0);
    }
    let mut end = 0;
    let mut count = 0;
    for (idx, ch) in text.char_indices() {
        if count == limit {
            break;
        }
        end = idx + ch.len_utf8();
        count += 1;
    }
    if count < limit {
        end = text.len();
    }
    let (head, tail) = text.split_at(end);
    (head, tail, count)
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
