use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::markdown::{
    heading_block_colors, line_row_span, markdown_to_render, HeadingOverlay, RenderedMarkdown,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub struct App {
    path: PathBuf,
    content: Vec<Line<'static>>,
    headings: Vec<HeadingOverlay>,
    scroll: u16,
    viewport_height: u16,
    status: Option<String>,
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
            status: Some(String::from("Press q to quit, arrows to scroll")),
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

        let paragraph = Paragraph::new(self.content.clone())
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .block(viewer_block);
        frame.render_widget(paragraph, viewport);

        self.highlight_headings(frame, inner);

        let status = Paragraph::new(self.status_line()).wrap(Wrap { trim: true });
        frame.render_widget(status, layout[1]);
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.scroll = self.scroll.saturating_sub(lines.max(1));
    }

    pub fn scroll_down(&mut self, lines: u16) {
        let max_scroll = self.max_scroll();
        self.scroll = (self.scroll + lines.max(1)).min(max_scroll);
    }

    pub fn scroll_to(&mut self, line: u16) {
        self.scroll = line.min(self.max_scroll());
    }

    pub fn scroll_to_end(&mut self) {
        self.scroll = self.max_scroll();
    }

    pub fn viewport_height(&self) -> u16 {
        self.viewport_height
    }

    pub fn set_status<T: Into<String>>(&mut self, msg: T) {
        self.status = Some(msg.into());
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

    fn highlight_headings(&self, frame: &mut Frame<'_>, inner: Rect) {
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
            "Space: page ↓  Shift+Space: page ↑  n/p: line  g/G: top/end  r: reload  q: quit",
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
