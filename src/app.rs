use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::markdown::{
    heading_block_colors, line_row_span, markdown_to_render, HeadingOverlay, RenderedMarkdown,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub struct App {
    path: PathBuf,
    content: Vec<Line<'static>>,
    headings: Vec<HeadingOverlay>,
    scroll: usize,
    viewport_height: u16,
    viewport_width: u16,
    status: Option<String>,
    show_help: bool,
}

impl App {
    pub fn load(path: &Path) -> io::Result<Self> {
        let markdown = fs::read_to_string(path)?;
        let render = markdown_to_render(&markdown);
        Ok(Self::new(path.to_path_buf(), render))
    }

    pub fn new(path: PathBuf, render: RenderedMarkdown) -> Self {
        Self {
            path,
            headings: render.headings,
            content: ensure_non_empty(render.lines),
            scroll: 0,
            viewport_height: 0,
            viewport_width: 80,
            status: Some(String::from("Press ? for help, q to quit")),
            show_help: false,
        }
    }

    pub fn reload(&mut self) -> io::Result<()> {
        let markdown = fs::read_to_string(&self.path)?;
        let render = markdown_to_render(&markdown);
        self.content = ensure_non_empty(render.lines);
        self.headings = render.headings;
        self.scroll = 0;
        Ok(())
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
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
        self.viewport_width = inner.width.max(1);

        let paragraph = Paragraph::new(self.content.clone())
            .wrap(Wrap { trim: false })
            .scroll((self.scroll as u16, 0))
            .block(viewer_block);
        frame.render_widget(paragraph, viewport);

        self.highlight_headings(frame, inner);

        let status = Paragraph::new(self.status_line()).wrap(Wrap { trim: true });
        frame.render_widget(status, layout[1]);

        if self.show_help {
            self.render_help(frame, frame.size());
        }
    }

    pub fn scroll_up(&mut self, rows: usize) {
        if rows == 0 {
            return;
        }
        self.scroll = self.scroll.saturating_sub(rows);
    }

    pub fn scroll_down(&mut self, rows: usize) {
        if rows == 0 {
            return;
        }
        self.scroll = self.scroll.saturating_add(rows).min(self.max_scroll());
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.viewport_height.max(1) as usize);
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.viewport_height.max(1) as usize);
    }

    pub fn scroll_to(&mut self, row: usize) {
        self.scroll = row.min(self.max_scroll());
    }

    pub fn scroll_to_end(&mut self) {
        self.scroll = self.max_scroll();
    }

    pub fn set_status<T: Into<String>>(&mut self, msg: T) {
        self.status = Some(msg.into());
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn is_help_open(&self) -> bool {
        self.show_help
    }

    fn max_scroll(&self) -> usize {
        self.total_rows()
            .saturating_sub(self.viewport_height as usize)
    }

    fn total_rows(&self) -> usize {
        let width = self.viewport_width.max(1) as usize;
        self.content
            .iter()
            .map(|line| line_row_span(line, width) as usize)
            .sum()
    }

    fn highlight_headings(&self, frame: &mut Frame<'_>, inner: Rect) {
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        let width = inner.width as usize;
        if width == 0 {
            return;
        }

        let visible_start_row = self.scroll;
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

    fn render_help(&self, frame: &mut Frame<'_>, area: Rect) {
        let popup = centered_rect(80, 80, area);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .title("Help (? / Esc to close)")
            .borders(Borders::ALL)
            .style(Style::default().bg(Color::Black));

        let mut lines = Vec::new();
        let header_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let bullet = |text: &str| Line::from(format!("  • {text}"));

        lines.push(Line::from(Span::styled("Navigation", header_style)));
        lines.push(bullet("Space / n: page down"));
        lines.push(bullet("p: page up"));
        lines.push(bullet("j / k or arrow keys: line scroll"));
        lines.push(bullet("PgUp / PgDn: page scroll"));
        lines.push(bullet("g or Home: top  |  G or End: bottom"));
        lines.push(bullet("r: reload file  |  q or Ctrl+C: quit"));
        lines.push(bullet("?: toggle this help overlay"));
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled("Heading Styles", header_style)));
        lines.push(bullet(
            "H1/H2 headings use tinted bands for major sections.",
        ));
        lines.push(bullet(
            "H3-H6 darken progressively to show nested hierarchy.",
        ));
        lines.push(bullet("Highlights span the full width behind the text."));
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled("Tips", header_style)));
        lines.push(bullet(
            "Edit in another window, press r to refresh instantly.",
        ));
        lines.push(bullet("Use Space/PgDn to skim; g/G jump to top/bottom."));
        lines.push(bullet("Arrow keys still work for fine-grained scrolling."));

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block);
        frame.render_widget(paragraph, popup);
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
            "Space or n: page ↓  p: page ↑  j/k: line  g/G: top/end  r: reload  q: quit",
        )];
        if let Some(status) = &self.status {
            spans.push(Span::raw("  -  "));
            spans.push(Span::styled(
                status.clone(),
                Style::default().fg(Color::Yellow),
            ));
        }
        Line::from(spans)
    }
}

fn ensure_non_empty(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if lines.is_empty() {
        lines.push(Line::from("(file is empty)"));
    }
    lines
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(vertical[1]);
    horizontal[1]
}
