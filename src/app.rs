use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::markdown::{
    heading_block_colors, line_row_span, markdown_to_render_with_options, CodeBlockOverlay,
    HeadingOverlay, MarkdownOptions, RenderedMarkdown, CODE_BLOCK_BG,
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
    source: String,
    content: Vec<Line<'static>>,
    headings: Vec<HeadingOverlay>,
    code_blocks: Vec<CodeBlockOverlay>,
    rules: Vec<usize>,
    table_width: usize,
    scroll: usize,
    viewport_height: u16,
    viewport_width: u16,
    status: Option<String>,
    show_help: bool,
}

impl App {
    pub fn load(path: &Path) -> io::Result<Self> {
        let markdown = fs::read_to_string(path)?;
        let options = MarkdownOptions::default();
        let render = markdown_to_render_with_options(&markdown, options);
        Ok(Self::new(
            path.to_path_buf(),
            markdown,
            render,
            options.max_table_width,
        ))
    }

    pub fn new(
        path: PathBuf,
        source: String,
        render: RenderedMarkdown,
        table_width: usize,
    ) -> Self {
        Self {
            path,
            source,
            headings: render.headings,
            code_blocks: render.code_blocks,
            rules: render.rules,
            content: ensure_non_empty(render.lines),
            table_width,
            scroll: 0,
            viewport_height: 0,
            viewport_width: 80,
            status: Some(String::from("Press ? for help, q to quit")),
            show_help: false,
        }
    }

    pub fn reload(&mut self) -> io::Result<()> {
        let markdown = fs::read_to_string(&self.path)?;
        let width = self.table_width.max(1);
        let options = MarkdownOptions {
            max_table_width: width,
        };
        let render = markdown_to_render_with_options(&markdown, options);
        self.source = markdown;
        self.apply_render(render);
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
        let width = inner.width.max(1) as usize;
        self.ensure_table_width(width);
        self.viewport_width = width as u16;
        let metrics = self.compute_line_metrics(self.viewport_width.max(1) as usize);

        let paragraph = Paragraph::new(self.content.clone())
            .wrap(Wrap { trim: false })
            .scroll((self.scroll as u16, 0))
            .block(viewer_block);
        frame.render_widget(paragraph, viewport);

        self.highlight_headings(frame, inner, &metrics);
        self.render_rules(frame, inner, &metrics);
        self.render_code_blocks(frame, inner, &metrics);

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

    fn compute_line_metrics(&self, width: usize) -> LineMetrics {
        let mut offsets = Vec::with_capacity(self.content.len() + 1);
        offsets.push(0);
        let mut total = 0usize;
        for line in &self.content {
            total += line_row_span(line, width) as usize;
            offsets.push(total);
        }
        LineMetrics { offsets }
    }

    fn ensure_table_width(&mut self, width: usize) {
        let width = width.max(1);
        if width == self.table_width {
            return;
        }
        let options = MarkdownOptions {
            max_table_width: width,
        };
        let render = markdown_to_render_with_options(&self.source, options);
        self.apply_render(render);
        self.table_width = width;
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn apply_render(&mut self, render: RenderedMarkdown) {
        self.content = ensure_non_empty(render.lines);
        self.headings = render.headings;
        self.code_blocks = render.code_blocks;
        self.rules = render.rules;
    }

    fn highlight_headings(&self, frame: &mut Frame<'_>, inner: Rect, metrics: &LineMetrics) {
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        let visible_start_row = self.scroll;
        let visible_end_row = visible_start_row + inner.height as usize;
        let buf = frame.buffer_mut();
        for heading in &self.headings {
            if heading.line >= self.content.len() {
                continue;
            }
            let Some((row_start, row_end)) = metrics.line_range(heading.line, heading.line + 1)
            else {
                continue;
            };
            if row_end <= row_start {
                continue;
            }
            if row_end <= visible_start_row || row_start >= visible_end_row {
                continue;
            }
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
    }

    fn render_code_blocks(&self, frame: &mut Frame<'_>, inner: Rect, metrics: &LineMetrics) {
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        let visible_start_row = self.scroll;
        let visible_end_row = visible_start_row + inner.height as usize;
        for block in &self.code_blocks {
            if block.line_start >= self.content.len() {
                continue;
            }
            let end_line = block.line_end.min(self.content.len());
            let Some((block_row_start, block_row_end)) =
                metrics.line_range(block.line_start, end_line)
            else {
                continue;
            };
            if block_row_end <= block_row_start {
                continue;
            }
            if block_row_end <= visible_start_row || block_row_start >= visible_end_row {
                continue;
            }
            let draw_start = block_row_start.max(visible_start_row);
            let draw_end = block_row_end.min(visible_end_row);
            let height_rows = draw_end.saturating_sub(draw_start);
            if height_rows == 0 {
                continue;
            }
            let area_y = inner.y + (draw_start - visible_start_row) as u16;
            let area = Rect {
                x: inner.x,
                y: area_y,
                width: inner.width,
                height: height_rows as u16,
            };
            let block_lines = self.content[block.line_start..end_line].to_vec();
            let block_scroll = (draw_start - block_row_start).min(u16::MAX as usize) as u16;
            let widget = Paragraph::new(block_lines)
                .wrap(Wrap { trim: false })
                .scroll((block_scroll, 0))
                .block(
                    Block::default()
                        .title(block.language.as_deref().unwrap_or("code"))
                        .borders(Borders::ALL)
                        .style(Style::default().bg(CODE_BLOCK_BG)),
                );
            frame.render_widget(widget, area);
        }
    }

    fn render_rules(&self, frame: &mut Frame<'_>, inner: Rect, metrics: &LineMetrics) {
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        let visible_start_row = self.scroll;
        let visible_end_row = visible_start_row + inner.height as usize;
        let buf = frame.buffer_mut();
        let style = Style::default().fg(Color::DarkGray);
        for &line_idx in &self.rules {
            if line_idx >= self.content.len() {
                continue;
            }
            let Some((row_start, row_end)) = metrics.line_range(line_idx, line_idx + 1) else {
                continue;
            };
            if row_end <= row_start {
                continue;
            }
            if row_end <= visible_start_row || row_start >= visible_end_row {
                continue;
            }
            let draw_start = row_start.max(visible_start_row);
            let draw_end = row_end.min(visible_end_row);
            for row in draw_start..draw_end {
                let offset = row - visible_start_row;
                if offset >= inner.height as usize {
                    break;
                }
                let y = inner.y + offset as u16;
                let x_end = inner.x.saturating_add(inner.width);
                for x in inner.x..x_end {
                    let cell = buf.get_mut(x, y);
                    cell.set_symbol("─");
                    cell.set_style(style);
                }
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

struct LineMetrics {
    offsets: Vec<usize>,
}

impl LineMetrics {
    fn line_range(&self, start_line: usize, end_line: usize) -> Option<(usize, usize)> {
        if end_line >= self.offsets.len() || start_line >= end_line {
            return None;
        }
        let start = *self.offsets.get(start_line)?;
        let end = *self.offsets.get(end_line)?;
        Some((start, end))
    }
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
