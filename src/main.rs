use std::{
    env,
    fs,
    io::{self, stdout},
    mem,
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, Event as MdEvent, Options, Parser, Tag};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

fn main() -> io::Result<()> {
    let Some(path) = env::args().nth(1).map(PathBuf::from) else {
        eprintln!("Usage: md-viewer <path-to-markdown>");
        std::process::exit(2);
    };

    let mut app = App::load(&path).map_err(|err| {
        io::Error::new(err.kind(), format!("failed to read {}: {err}", path.display()))
    })?;

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

fn run(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, app: &mut App) -> io::Result<()> {
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
        KeyCode::Up | KeyCode::Char('k') => app.scroll_up(1),
        KeyCode::Down | KeyCode::Char('j') => app.scroll_down(1),
        KeyCode::PageUp => app.scroll_up(app.viewport_height.saturating_sub(1)),
        KeyCode::PageDown => app.scroll_down(app.viewport_height.saturating_sub(1)),
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

struct App {
    path: PathBuf,
    content: Vec<Line<'static>>,
    headings: Vec<HeadingOverlay>,
    scroll: u16,
    viewport_height: u16,
    status: Option<String>,
}

impl App {
    fn load(path: &Path) -> io::Result<Self> {
        let markdown = fs::read_to_string(path)?;
        let render = markdown_to_render(&markdown);
        Ok(Self {
            path: path.to_path_buf(),
            headings: render.headings,
            content: ensure_non_empty(render.lines),
            scroll: 0,
            viewport_height: 0,
            status: Some(String::from("Press q to quit, arrows to scroll")),
        })
    }

    fn reload(&mut self) -> io::Result<()> {
        let markdown = fs::read_to_string(&self.path)?;
        let render = markdown_to_render(&markdown);
        self.content = ensure_non_empty(render.lines);
        self.headings = render.headings;
        self.scroll = 0;
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(frame.size());

        let viewer_block = Block::default()
            .title(self.title_line())
            .borders(Borders::ALL);

        let viewport = layout[0];
        let inner = viewer_block.inner(viewport);
        self.viewport_height = inner.height.max(1);

        let paragraph = Paragraph::new(self.content.clone())
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .block(viewer_block);
        frame.render_widget(paragraph, viewport);

        self.highlight_headings(frame, inner);

        let status = Paragraph::new(self.status_line()).wrap(Wrap { trim: true });
        frame.render_widget(status, layout[1]);
    }

    fn title_line(&self) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("{}", self.path.display()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" "),
            Span::styled(
                format!("({} lines)", self.content.len()),
                Style::default().fg(Color::Gray),
            ),
        ])
    }

    fn status_line(&self) -> Line<'static> {
        let mut spans = vec![Span::raw(
            "Up/Down: scroll  PgUp/PgDn: jump  g/G: top/end  r: reload  q: quit",
        )];
        if let Some(status) = &self.status {
            spans.push(Span::raw("  -  "));
            spans.push(Span::styled(status.clone(), Style::default().fg(Color::Yellow)));
        }
        Line::from(spans)
    }

    fn scroll_up(&mut self, lines: u16) {
        self.scroll = self.scroll.saturating_sub(lines.max(1));
    }

    fn scroll_down(&mut self, lines: u16) {
        let max_scroll = self.max_scroll();
        self.scroll = (self.scroll + lines.max(1)).min(max_scroll);
    }

    fn scroll_to(&mut self, line: u16) {
        self.scroll = line.min(self.max_scroll());
    }

    fn scroll_to_end(&mut self) {
        self.scroll = self.max_scroll();
    }

    fn max_scroll(&self) -> u16 {
        if self.viewport_height == 0 {
            return 0;
        }
        let content_height = self.content.len() as i32;
        let viewport = self.viewport_height as i32;
        if content_height <= viewport {
            0
        } else {
            (content_height - viewport) as u16
        }
    }

    fn set_status<T: Into<String>>(&mut self, msg: T) {
        self.status = Some(msg.into());
    }

    fn highlight_headings(&self, frame: &mut Frame, inner: Rect) {
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        let width = inner.width as usize;
        if width == 0 {
            return;
        }

        let visible_start_row = self.scroll as usize;
        let visible_end_row = visible_start_row + inner.height as usize;
        let mut heading_iter = self.headings.iter().peekable();
        if heading_iter.peek().is_none() {
            return;
        }

        let mut row_cursor = 0usize;
        let buf = frame.buffer_mut();
        for (line_idx, line) in self.content.iter().enumerate() {
            let span_rows = line_row_span(line, width) as usize;
            if span_rows == 0 {
                continue;
            }
            while let Some(heading) = heading_iter.peek() {
                if heading.line == line_idx {
                    let row_start = row_cursor;
                    let row_end = row_cursor + span_rows;
                    if row_end > visible_start_row && row_start < visible_end_row {
                        let paint_start = row_start.max(visible_start_row) - visible_start_row;
                        let paint_end = row_end.min(visible_end_row) - visible_start_row;
                        let (bg, _) = heading_block_colors(heading.level);
                        for offset in paint_start..paint_end {
                            if offset >= inner.height as usize {
                                break;
                            }
                            let y = inner.y + offset as u16;
                            let x_end = inner.x.saturating_add(inner.width);
                            for x in inner.x..x_end {
                                buf.get_mut(x, y).set_bg(bg);
                            }
                        }
                    }
                    heading_iter.next();
                } else {
                    break;
                }
            }
            row_cursor += span_rows;
            if row_cursor >= visible_end_row && heading_iter.peek().is_none() {
                break;
            }
        }
    }
}

fn line_row_span(line: &Line<'_>, width: usize) -> u16 {
    if width == 0 {
        return 0;
    }
    let line_width = line.width();
    if line_width == 0 {
        1
    } else {
        let rows = (line_width + width - 1) / width;
        rows.min(u16::MAX as usize) as u16
    }
}

fn ensure_non_empty(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if lines.is_empty() {
        lines.push(Line::from("(file is empty)"));
    }
    lines
}

fn markdown_to_render(markdown: &str) -> RenderedMarkdown {
    let mut buffer = MarkdownBuffer::default();
    let parser = Parser::new_ext(
        markdown,
        Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TABLES
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES,
    );
    for event in parser {
        buffer.handle_event(event);
    }
    buffer.finalize()
}

struct RenderedMarkdown {
    lines: Vec<Line<'static>>,
    headings: Vec<HeadingOverlay>,
}

#[derive(Clone, Copy)]
struct HeadingOverlay {
    line: usize,
    level: pulldown_cmark::HeadingLevel,
}

struct MarkdownBuffer {
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    list_stack: Vec<ListState>,
    blockquote_depth: usize,
    in_code_block: bool,
    line_start: bool,
    last_blank: bool,
    table: Option<TableBuilder>,
    heading_overlays: Vec<HeadingOverlay>,
    pending_heading: Option<pulldown_cmark::HeadingLevel>,
}

impl Default for MarkdownBuffer {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            current: Vec::new(),
            style_stack: vec![Style::default()],
            list_stack: Vec::new(),
            blockquote_depth: 0,
            in_code_block: false,
            line_start: true,
            last_blank: true,
            table: None,
            heading_overlays: Vec::new(),
            pending_heading: None,
        }
    }
}

#[derive(Clone, Copy)]
struct ListState {
    ordered: bool,
    index: usize,
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            ordered: false,
            index: 1,
        }
    }
}

impl MarkdownBuffer {
    fn start_heading(&mut self, level: pulldown_cmark::HeadingLevel) {
        self.ensure_block_gap();
        self.pending_heading = Some(level);
        self.push_style(self.heading_text_style(level));
    }

    fn end_heading(&mut self, _level: pulldown_cmark::HeadingLevel) {
        self.flush_line(false);
        self.push_blank_line();
        self.pop_style();
    }

    fn handle_event(&mut self, event: MdEvent<'_>) {
        match event {
            MdEvent::Start(tag) => self.start_tag(tag),
            MdEvent::End(tag) => self.end_tag(tag),
            MdEvent::Text(text) => {
                if self.push_table_text(&text) {
                    return;
                }
                self.push_text(text)
            }
            MdEvent::Code(code) => {
                if self.push_table_code(&code) {
                    return;
                }
                self.push_code_span(code)
            }
            MdEvent::Html(html) => {
                if self.push_table_text(&html) {
                    return;
                }
                self.push_text(html)
            }
            MdEvent::SoftBreak => {
                if self.push_table_soft_break() {
                    return;
                }
                self.soft_break()
            }
            MdEvent::HardBreak => {
                if self.push_table_hard_break() {
                    return;
                }
                self.hard_break()
            }
            MdEvent::Rule => self.push_rule(),
            MdEvent::FootnoteReference(reference) => {
                let footnote = CowStr::from(format!("[^{reference}]"));
                if self.push_table_text(&footnote) {
                    return;
                }
                self.push_text(footnote);
            }
            MdEvent::TaskListMarker(done) => {
                let marker = if done { "[x] " } else { "[ ] " };
                let marker = CowStr::from(marker.to_string());
                if self.push_table_text(&marker) {
                    return;
                }
                self.push_text(marker);
            }
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Table(alignments) => {
                self.ensure_block_gap();
                self.flush_line(false);
                self.table = Some(TableBuilder::new(alignments));
                return;
            }
            Tag::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.start_head();
                }
                return;
            }
            Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.start_row();
                }
                return;
            }
            Tag::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    table.start_cell();
                }
                return;
            }
            _ => {}
        }

        if self.table_cell_active() {
            match tag {
                Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Link(_, _, _)
                | Tag::Paragraph => return,
                _ => {}
            }
        }

        match tag {
            Tag::Paragraph => self.ensure_block_gap(),
            Tag::Heading(level, _, _) => {
                self.start_heading(level);
            }
            Tag::BlockQuote => {
                self.ensure_block_gap();
                self.blockquote_depth += 1;
                self.push_style(
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::ITALIC),
                );
            }
            Tag::List(start) => {
                self.list_stack.push(ListState {
                    ordered: start.is_some(),
                    index: start.unwrap_or(1) as usize,
                });
            }
            Tag::Item => self.start_list_item(),
            Tag::CodeBlock(kind) => self.start_code_block(kind),
            Tag::Emphasis => self.push_style(self.current_style().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(self.current_style().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => {
                self.push_style(self.current_style().add_modifier(Modifier::CROSSED_OUT))
            }
            Tag::Link(_, _, _) => {
                self.push_style(
                    self.current_style()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                )
            }
            Tag::Image(_, dest, title) => {
                self.ensure_block_gap();
                let label = if title.is_empty() {
                    format!("![image]({})", dest)
                } else {
                    format!("![{}]({})", title, dest)
                };
                self.push_text(label.into());
                self.soft_break();
            }
            Tag::FootnoteDefinition(name) => {
                self.ensure_block_gap();
                self.push_text(CowStr::from(format!("[^{name}]")));
                self.soft_break();
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    table.end_cell();
                }
                return;
            }
            Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.end_row();
                }
                return;
            }
            Tag::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.end_head();
                }
                return;
            }
            Tag::Table(_) => {
                self.flush_line(false);
                if let Some(table) = self.table.take() {
                    let mut rendered = table.into_lines();
                    if rendered.is_empty() {
                        rendered.push(Line::from("(empty table)"));
                    }
                    self.lines.extend(rendered);
                    self.push_blank_line();
                }
                return;
            }
            _ => {}
        }

        if self.table_cell_active() {
            match tag {
                Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link(_, _, _) => return,
                _ => {}
            }
        }

        match tag {
            Tag::Paragraph => {
                self.flush_line(false);
                self.push_blank_line();
            }
            Tag::Heading(level, _, _) => {
                self.end_heading(level);
            }
            Tag::BlockQuote => {
                self.flush_line(false);
                if self.blockquote_depth > 0 {
                    self.blockquote_depth -= 1;
                }
                self.pop_style();
                self.push_blank_line();
            }
            Tag::List(_) => {
                self.flush_line(false);
                self.list_stack.pop();
                self.push_blank_line();
            }
            Tag::Item => {
                self.flush_line(false);
            }
            Tag::CodeBlock(_) => {
                self.flush_line(false);
                self.pop_style();
                self.in_code_block = false;
                self.push_blank_line();
            }
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link(_, _, _) => {
                self.pop_style();
            }
            Tag::Image(..) | Tag::FootnoteDefinition(_) => {}
            _ => {}
        }
    }

    fn start_code_block(&mut self, kind: CodeBlockKind<'_>) {
        self.ensure_block_gap();
        if let CodeBlockKind::Fenced(info) = kind {
            let info = info.trim();
            if !info.is_empty() {
                self.push_text(CowStr::from(format!("```{info}")));
                self.soft_break();
            }
        }
        self.in_code_block = true;
        self.push_style(Style::default().fg(Color::Yellow));
    }

    fn start_list_item(&mut self) {
        self.flush_line(false);
        let indent = self.list_stack.len().saturating_sub(1) * 2;
        let padding = " ".repeat(indent);
        if let Some(state) = self.list_stack.last_mut() {
            let bullet = if state.ordered {
                let label = format!("{}{}. ", padding, state.index);
                state.index += 1;
                label
            } else {
                format!("{}- ", padding)
            };
            self.current.push(Span::styled(
                bullet,
                Style::default().fg(Color::Gray),
            ));
            self.line_start = false;
        } else {
            self.current
                .push(Span::styled("- ", Style::default().fg(Color::Gray)));
            self.line_start = false;
        }
    }

    fn push_style(&mut self, style: Style) {
        self.style_stack.push(style);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn current_style(&self) -> Style {
        self.style_stack
            .last()
            .copied()
            .unwrap_or_else(Style::default)
    }

    fn push_text(&mut self, text: CowStr<'_>) {
        if text.is_empty() {
            return;
        }
        if self.in_code_block && text.contains('\n') {
            let mut buffer = String::new();
            for ch in text.chars() {
                if ch == '\n' {
                    self.push_text_segment(&buffer);
                    buffer.clear();
                    self.flush_line(true);
                } else {
                    buffer.push(ch);
                }
            }
            if !buffer.is_empty() {
                self.push_text_segment(&buffer);
            }
            return;
        }
        self.push_text_segment(text.as_ref());
    }

    fn push_text_segment(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.style_stack.is_empty() {
            self.style_stack.push(Style::default());
        }
        if self.line_start {
            self.insert_prefixes();
        }
        let style = self.current_style();
        self.current
            .push(Span::styled(text.to_string(), style));
        self.last_blank = false;
    }

    fn push_code_span(&mut self, text: CowStr<'_>) {
        let style = self
            .current_style()
            .fg(Color::Yellow)
            .add_modifier(Modifier::DIM);
        if self.line_start {
            self.insert_prefixes();
        }
        self.current
            .push(Span::styled(format!("`{}`", text), style));
        self.last_blank = false;
    }

    fn table_cell_active(&self) -> bool {
        self.table
            .as_ref()
            .map(TableBuilder::is_collecting)
            .unwrap_or(false)
    }

    fn push_table_text(&mut self, text: &CowStr<'_>) -> bool {
        if let Some(table) = self.table.as_mut() {
            if table.is_collecting() {
                table.push_text(text);
                return true;
            }
        }
        false
    }

    fn push_table_code(&mut self, text: &CowStr<'_>) -> bool {
        if let Some(table) = self.table.as_mut() {
            if table.is_collecting() {
                table.push_code(text);
                return true;
            }
        }
        false
    }

    fn push_table_soft_break(&mut self) -> bool {
        if let Some(table) = self.table.as_mut() {
            if table.is_collecting() {
                table.push_soft_break();
                return true;
            }
        }
        false
    }

    fn push_table_hard_break(&mut self) -> bool {
        if let Some(table) = self.table.as_mut() {
            if table.is_collecting() {
                table.push_hard_break();
                return true;
            }
        }
        false
    }

    fn insert_prefixes(&mut self) {
        if self.in_code_block {
            self.current.push(Span::raw("    "));
        }
        if self.blockquote_depth > 0 {
            let mut prefix = String::new();
            for _ in 0..self.blockquote_depth {
                prefix.push_str("> ");
            }
            self.current.push(Span::styled(
                prefix,
                Style::default().fg(Color::DarkGray),
            ));
        }
        self.line_start = false;
    }

    fn soft_break(&mut self) {
        self.flush_line(false);
    }

    fn hard_break(&mut self) {
        self.flush_line(true);
    }

    fn flush_line(&mut self, allow_empty: bool) {
        if self.current.is_empty() {
            if allow_empty {
                self.lines.push(Line::default());
                self.last_blank = true;
            }
        } else {
            let spans = mem::take(&mut self.current);
            self.lines.push(Line::from(spans));
            self.last_blank = false;
            if let Some(level) = self.pending_heading.take() {
                let line_index = self.lines.len().saturating_sub(1);
                self.heading_overlays.push(HeadingOverlay {
                    line: line_index,
                    level,
                });
            }
        }
        self.line_start = true;
    }

    fn ensure_block_gap(&mut self) {
        if !self.lines.is_empty() && !self.last_blank {
            self.lines.push(Line::default());
            self.last_blank = true;
        }
        self.line_start = true;
    }

    fn push_blank_line(&mut self) {
        if !self.last_blank {
            self.lines.push(Line::default());
            self.last_blank = true;
        }
        self.line_start = true;
    }

    fn push_rule(&mut self) {
        self.ensure_block_gap();
        self.lines.push(Line::from(vec![Span::styled(
            "-".repeat(20),
            Style::default().fg(Color::DarkGray),
        )]));
        self.lines.push(Line::default());
        self.last_blank = true;
        self.line_start = true;
    }

    fn finalize(mut self) -> RenderedMarkdown {
        if !self.current.is_empty() {
            let spans = mem::take(&mut self.current);
            self.lines.push(Line::from(spans));
        }
        RenderedMarkdown {
            lines: self.lines,
            headings: self.heading_overlays,
        }
    }
}

impl MarkdownBuffer {
    fn heading_text_style(&self, level: pulldown_cmark::HeadingLevel) -> Style {
        match level {
            pulldown_cmark::HeadingLevel::H1 => Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            pulldown_cmark::HeadingLevel::H2 => Style::default()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
            pulldown_cmark::HeadingLevel::H3 => Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
            pulldown_cmark::HeadingLevel::H4 => Style::default().fg(Color::Magenta),
            pulldown_cmark::HeadingLevel::H5 => Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::ITALIC),
            pulldown_cmark::HeadingLevel::H6 => Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
        }
    }
}

fn heading_block_colors(level: pulldown_cmark::HeadingLevel) -> (Color, Color) {
    match level {
        pulldown_cmark::HeadingLevel::H1 => (Color::Rgb(48, 52, 70), Color::Rgb(235, 235, 245)),
        pulldown_cmark::HeadingLevel::H2 => (Color::Rgb(40, 44, 60), Color::Rgb(225, 225, 235)),
        pulldown_cmark::HeadingLevel::H3 => (Color::Rgb(35, 39, 54), Color::Rgb(210, 210, 225)),
        pulldown_cmark::HeadingLevel::H4 => (Color::Rgb(30, 34, 48), Color::Rgb(200, 200, 215)),
        pulldown_cmark::HeadingLevel::H5 => (Color::Rgb(28, 32, 44), Color::Rgb(190, 190, 205)),
        pulldown_cmark::HeadingLevel::H6 => (Color::Rgb(24, 28, 38), Color::Rgb(180, 180, 195)),
    }
}

struct TableBuilder {
    alignments: Vec<Alignment>,
    header: Option<Vec<String>>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    in_head: bool,
    in_cell: bool,
}

impl TableBuilder {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            header: None,
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            in_head: false,
            in_cell: false,
        }
    }

    fn start_head(&mut self) {
        self.in_head = true;
    }

    fn end_head(&mut self) {
        self.in_head = false;
    }

    fn start_row(&mut self) {
        self.current_row.clear();
    }

    fn end_row(&mut self) {
        if self.in_cell {
            self.end_cell();
        }
        if self.current_row.is_empty() {
            return;
        }
        if self.in_head && self.header.is_none() {
            self.header = Some(self.current_row.clone());
        } else {
            self.rows.push(self.current_row.clone());
        }
        self.current_row.clear();
    }

    fn start_cell(&mut self) {
        if self.in_cell {
            self.end_cell();
        }
        self.current_cell.clear();
        self.in_cell = true;
    }

    fn end_cell(&mut self) {
        if !self.in_cell {
            return;
        }
        let text = self.current_cell.trim().to_string();
        self.current_row.push(text);
        self.current_cell.clear();
        self.in_cell = false;
    }

    fn push_text(&mut self, text: &CowStr<'_>) {
        if !self.in_cell {
            return;
        }
        self.current_cell.push_str(text.as_ref());
    }

    fn push_code(&mut self, text: &CowStr<'_>) {
        if !self.in_cell {
            return;
        }
        self.current_cell.push('`');
        self.current_cell.push_str(text.as_ref());
        self.current_cell.push('`');
    }

    fn push_soft_break(&mut self) {
        if !self.in_cell {
            return;
        }
        if !self.current_cell.ends_with(' ') {
            self.current_cell.push(' ');
        }
    }

    fn push_hard_break(&mut self) {
        if !self.in_cell {
            return;
        }
        self.current_cell.push(' ');
    }

    fn is_collecting(&self) -> bool {
        self.in_cell
    }

    fn into_lines(mut self) -> Vec<Line<'static>> {
        if self.in_cell {
            self.end_cell();
        }
        if !self.current_row.is_empty() {
            self.end_row();
        }

        let mut col_count = self.alignments.len();
        if let Some(header) = &self.header {
            col_count = col_count.max(header.len());
        }
        for row in &self.rows {
            col_count = col_count.max(row.len());
        }
        if col_count == 0 {
            return Vec::new();
        }

        if self.alignments.len() < col_count {
            self.alignments.resize(col_count, Alignment::Left);
        }

        let mut widths = vec![3; col_count];
        if let Some(header) = &self.header {
            update_widths(&mut widths, header);
        }
        for row in &self.rows {
            update_widths(&mut widths, row);
        }

        let mut lines = Vec::new();
        lines.push(Line::from(table_separator(&widths)));
        if let Some(header) = &self.header {
            lines.push(Line::from(build_row(header, &widths, &self.alignments)));
            lines.push(Line::from(table_separator(&widths)));
        }
        for row in &self.rows {
            lines.push(Line::from(build_row(row, &widths, &self.alignments)));
        }
        lines.push(Line::from(table_separator(&widths)));
        lines
    }
}

fn update_widths(widths: &mut [usize], row: &[String]) {
    for (idx, cell) in row.iter().enumerate() {
        if idx < widths.len() {
            widths[idx] = widths[idx].max(display_width(cell));
        }
    }
}

fn display_width(value: &str) -> usize {
    value.chars().count().max(1)
}

fn build_row(row: &[String], widths: &[usize], alignments: &[Alignment]) -> String {
    let mut line = String::new();
    line.push('|');
    for (idx, width) in widths.iter().enumerate() {
        let cell = row.get(idx).map(|s| s.as_str()).unwrap_or("");
        line.push(' ');
        line.push_str(&pad_cell(cell, *width, alignments[idx]));
        line.push(' ');
        line.push('|');
    }
    line
}

fn pad_cell(text: &str, width: usize, alignment: Alignment) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return " ".repeat(width);
    }
    match alignment {
        Alignment::Right => format!("{:>width$}", trimmed),
        Alignment::Center => {
            let len = trimmed.chars().count();
            if len >= width {
                return trimmed.to_string();
            }
            let padding = width - len;
            let left = padding / 2;
            let right = padding - left;
            format!("{}{}{}", " ".repeat(left), trimmed, " ".repeat(right))
        }
        _ => format!("{: <width$}", trimmed),
    }
}

fn table_separator(widths: &[usize]) -> String {
    let mut line = String::new();
    line.push('+');
    for width in widths {
        line.push_str(&"-".repeat(width + 2));
        line.push('+');
    }
    line
}
