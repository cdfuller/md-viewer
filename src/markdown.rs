use std::mem;

use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, Event as MdEvent, Options, Parser, Tag};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub const CODE_BLOCK_FG: Color = Color::Rgb(225, 228, 235);
pub const CODE_BLOCK_BG: Color = Color::Rgb(12, 16, 26);
pub const CODE_BLOCK_BORDER_FG: Color = Color::Rgb(150, 160, 175);

pub fn markdown_to_render(markdown: &str) -> RenderedMarkdown {
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

pub fn heading_block_colors(level: pulldown_cmark::HeadingLevel) -> (Color, Color) {
    match level {
        pulldown_cmark::HeadingLevel::H1 => (Color::Rgb(48, 52, 70), Color::Rgb(235, 235, 245)),
        pulldown_cmark::HeadingLevel::H2 => (Color::Rgb(40, 44, 60), Color::Rgb(225, 225, 235)),
        pulldown_cmark::HeadingLevel::H3 => (Color::Rgb(35, 39, 54), Color::Rgb(210, 210, 225)),
        pulldown_cmark::HeadingLevel::H4 => (Color::Rgb(30, 34, 48), Color::Rgb(200, 200, 215)),
        pulldown_cmark::HeadingLevel::H5 => (Color::Rgb(28, 32, 44), Color::Rgb(190, 190, 205)),
        pulldown_cmark::HeadingLevel::H6 => (Color::Rgb(24, 28, 38), Color::Rgb(180, 180, 195)),
    }
}

pub fn line_row_span(line: &Line<'_>, width: usize) -> u16 {
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

pub struct RenderedMarkdown {
    pub lines: Vec<Line<'static>>,
    pub headings: Vec<HeadingOverlay>,
    pub code_blocks: Vec<CodeBlockOverlay>,
}

#[derive(Clone, Copy)]
pub struct HeadingOverlay {
    pub line: usize,
    pub level: pulldown_cmark::HeadingLevel,
}

#[derive(Clone)]
pub struct CodeBlockOverlay {
    pub line_start: usize,
    pub line_end: usize,
    pub language: Option<String>,
}

struct MarkdownBuffer {
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    list_stack: Vec<ListState>,
    blockquote_depth: usize,
    in_code_block: bool,
    code_block_start: Option<usize>,
    code_block_language: Option<String>,
    line_start: bool,
    last_blank: bool,
    table: Option<TableBuilder>,
    heading_overlays: Vec<HeadingOverlay>,
    code_blocks: Vec<CodeBlockOverlay>,
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
            code_block_start: None,
            code_block_language: None,
            line_start: true,
            last_blank: true,
            table: None,
            heading_overlays: Vec::new(),
            code_blocks: Vec::new(),
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
                self.ensure_block_gap();
                self.pending_heading = Some(level);
                self.push_style(self.heading_text_style(level));
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
            Tag::Link(_, _, _) => self.push_style(
                self.current_style()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ),
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
            Tag::Heading(..) => {
                self.flush_line(false);
                self.push_blank_line();
                self.pop_style();
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
                self.finish_code_block();
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
        self.flush_line(false);
        self.code_block_language = match kind {
            CodeBlockKind::Fenced(info) => {
                let trimmed = info.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            CodeBlockKind::Indented => None,
        };
        self.code_block_start = Some(self.lines.len());
        self.in_code_block = true;
        self.push_style(Self::code_block_style());
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
            self.current
                .push(Span::styled(bullet, Style::default().fg(Color::Gray)));
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
        self.current.push(Span::styled(text.to_string(), style));
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
            self.current
                .push(Span::styled("    ", Self::code_block_style()));
        }
        if self.blockquote_depth > 0 {
            let mut prefix = String::new();
            for _ in 0..self.blockquote_depth {
                prefix.push_str("> ");
            }
            let mut style = Style::default().fg(Color::DarkGray);
            if self.in_code_block {
                style = style.bg(CODE_BLOCK_BG);
            }
            self.current.push(Span::styled(prefix, style));
        }
        self.line_start = false;
    }

    fn finish_code_block(&mut self) {
        let Some(start) = self.code_block_start.take() else {
            self.code_block_language = None;
            return;
        };
        if start > self.lines.len() {
            self.code_block_language = None;
            return;
        }
        if start == self.lines.len() {
            self.lines.push(Line::default());
        }
        let end = self.lines.len();
        if end > start {
            self.code_blocks.push(CodeBlockOverlay {
                line_start: start,
                line_end: end,
                language: self.code_block_language.take(),
            });
        } else {
            self.code_block_language = None;
        }
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
            code_blocks: self.code_blocks,
        }
    }

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

impl MarkdownBuffer {
    fn code_block_style() -> Style {
        Style::default().fg(CODE_BLOCK_FG).bg(CODE_BLOCK_BG)
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
                trimmed.to_string()
            } else {
                let padding = width - len;
                let left = padding / 2;
                let right = padding - left;
                format!("{}{}{}", " ".repeat(left), trimmed, " ".repeat(right))
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_overlays_record_line_numbers() {
        let markdown = "# Title\n\nSome text\n\n## Section\ncontent\n";
        let render = markdown_to_render(markdown);
        assert_eq!(render.headings.len(), 2);
        assert_eq!(render.headings[0].line, 0);
        assert!(matches!(
            render.headings[0].level,
            pulldown_cmark::HeadingLevel::H1
        ));
        assert!(matches!(
            render.headings[1].level,
            pulldown_cmark::HeadingLevel::H2
        ));
    }

    #[test]
    fn table_builder_outputs_ascii_rows() {
        let markdown = "| A | B |\n|---|---|\n| x | y |";
        let render = markdown_to_render(markdown);
        let joined: String = render
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("+"));
        assert!(joined.contains("| x"));
    }

    #[test]
    fn line_row_span_accounts_for_wrapping() {
        let line = Line::from("abcdefghij");
        assert_eq!(line_row_span(&line, 20), 1);
        assert_eq!(line_row_span(&line, 5), 2);
        assert_eq!(line_row_span(&line, 3), 4);
    }

    #[test]
    fn task_list_markers_render() {
        let markdown = "- [x] done\n- [ ] todo";
        let render = markdown_to_render(markdown);
        let combined: String = render
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(combined.contains("[x] done"));
        assert!(combined.contains("[ ] todo"));
    }

    #[test]
    fn ordered_list_increments() {
        let markdown = "1. first\n2. second";
        let render = markdown_to_render(markdown);
        let text: String = render
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("1. first"));
        assert!(text.contains("2. second"));
    }

    #[test]
    fn code_block_overlay_records_language_and_lines() {
        let markdown = "before\n```rust\nfn main() {}\n```\nafter\n";
        let render = markdown_to_render(markdown);
        assert_eq!(render.code_blocks.len(), 1);
        let block = &render.code_blocks[0];
        assert_eq!(block.language.as_deref(), Some("rust"));
        assert!(block.line_end > block.line_start);
        let code_slice = &render.lines[block.line_start..block.line_end];
        let combined: String = code_slice
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(combined.contains("fn main() {}"));
    }

    #[test]
    fn code_block_lines_use_background_color() {
        let markdown = "```\nlet x = 42;\n```\n";
        let render = markdown_to_render(markdown);
        let code_line = render
            .lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("let x = 42;"))
            })
            .expect("code line rendered");
        assert!(!code_line.spans.is_empty());
        for span in &code_line.spans {
            assert_eq!(span.style.bg, Some(CODE_BLOCK_BG));
        }
    }

    #[test]
    fn heading_block_colors_are_distinct() {
        let colors: Vec<_> = [
            pulldown_cmark::HeadingLevel::H1,
            pulldown_cmark::HeadingLevel::H2,
            pulldown_cmark::HeadingLevel::H3,
            pulldown_cmark::HeadingLevel::H4,
            pulldown_cmark::HeadingLevel::H5,
            pulldown_cmark::HeadingLevel::H6,
        ]
        .iter()
        .map(|lvl| heading_block_colors(*lvl))
        .collect();
        assert!(colors.windows(2).all(|pair| pair[0] != pair[1]));
    }
}
