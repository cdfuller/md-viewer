use std::mem;

use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, Event as MdEvent, Options, Parser, Tag};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use textwrap::{wrap, Options as WrapOptions};
use unicode_width::UnicodeWidthStr;

pub const CODE_BLOCK_FG: Color = Color::Rgb(225, 228, 235);
pub const CODE_BLOCK_BG: Color = Color::Rgb(12, 16, 26);
pub const CODE_BLOCK_BORDER_FG: Color = Color::Rgb(150, 160, 175);
const MIN_COLUMN_WIDTH: usize = 3;

#[derive(Clone, Copy)]
pub struct MarkdownOptions {
    pub max_table_width: usize,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            max_table_width: 80,
        }
    }
}

pub fn markdown_to_render(markdown: &str) -> RenderedMarkdown {
    markdown_to_render_with_options(markdown, MarkdownOptions::default())
}

pub fn markdown_to_render_with_options(
    markdown: &str,
    options: MarkdownOptions,
) -> RenderedMarkdown {
    let mut buffer = MarkdownBuffer::new(options);
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
    pub rules: Vec<usize>,
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

struct LineWriter {
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    line_start: bool,
    last_blank: bool,
    pending_heading: Option<pulldown_cmark::HeadingLevel>,
    heading_overlays: Vec<HeadingOverlay>,
}

impl Default for LineWriter {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            current: Vec::new(),
            line_start: true,
            last_blank: true,
            pending_heading: None,
            heading_overlays: Vec::new(),
        }
    }
}

impl LineWriter {
    fn is_line_start(&self) -> bool {
        self.line_start
    }

    fn len(&self) -> usize {
        self.lines.len()
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    fn queue_heading(&mut self, level: pulldown_cmark::HeadingLevel) {
        self.pending_heading = Some(level);
    }

    fn push_span(&mut self, span: Span<'static>, mark_content: bool) {
        self.current.push(span);
        if mark_content {
            self.last_blank = false;
        }
        self.line_start = false;
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
        if !self.is_empty() && !self.last_blank {
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

    fn push_manual_line(&mut self, line: Line<'static>) -> usize {
        let idx = self.lines.len();
        self.lines.push(line);
        self.last_blank = self.lines[idx].spans.is_empty();
        self.line_start = true;
        idx
    }

    fn mark_last_non_blank(&mut self) {
        self.last_blank = false;
    }

    fn extend_lines(&mut self, new_lines: Vec<Line<'static>>) {
        if new_lines.is_empty() {
            return;
        }
        self.lines.extend(new_lines);
        self.last_blank = self
            .lines
            .last()
            .map(|line| line.spans.is_empty())
            .unwrap_or(true);
        self.line_start = true;
    }

    fn finalize(mut self) -> (Vec<Line<'static>>, Vec<HeadingOverlay>) {
        if !self.current.is_empty() {
            let spans = mem::take(&mut self.current);
            self.lines.push(Line::from(spans));
        }
        (self.lines, self.heading_overlays)
    }
}

#[derive(Default)]
struct CodeBlockState {
    start_line: Option<usize>,
    language: Option<String>,
}

impl CodeBlockState {
    fn is_active(&self) -> bool {
        self.start_line.is_some()
    }

    fn start(&mut self, start_line: usize, language: Option<String>) {
        self.start_line = Some(start_line);
        self.language = language;
    }

    fn take(&mut self) -> Option<(usize, Option<String>)> {
        self.start_line
            .take()
            .map(|start| (start, self.language.take()))
    }
}

struct MarkdownBuffer {
    lines: LineWriter,
    style_stack: Vec<Style>,
    list_stack: Vec<ListState>,
    blockquote_depth: usize,
    table: Option<TableBuilder>,
    code_blocks: Vec<CodeBlockOverlay>,
    rule_lines: Vec<usize>,
    code_block: CodeBlockState,
    options: MarkdownOptions,
}

impl MarkdownBuffer {
    fn new(options: MarkdownOptions) -> Self {
        Self {
            lines: LineWriter::default(),
            style_stack: vec![Style::default()],
            list_stack: Vec::new(),
            blockquote_depth: 0,
            table: None,
            code_blocks: Vec::new(),
            rule_lines: Vec::new(),
            code_block: CodeBlockState::default(),
            options,
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
                if self.push_table_html(&html) {
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
                self.table = Some(TableBuilder::new(alignments, self.options.max_table_width));
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
                self.lines.queue_heading(level);
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
                    self.lines.extend_lines(rendered);
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
        let language = match kind {
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
        let start = self.lines.len();
        self.code_block.start(start, language);
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
                let symbol = unordered_bullet(self.list_stack.len() - 1);
                format!("{}{} ", padding, symbol)
            };
            self.lines.push_span(
                Span::styled(bullet, Style::default().fg(Color::Gray)),
                false,
            );
        } else {
            self.lines
                .push_span(Span::styled("- ", Style::default().fg(Color::Gray)), false);
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
        if self.code_block.is_active() && text.contains('\n') {
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
        if self.lines.is_line_start() {
            self.insert_prefixes();
        }
        let style = self.current_style();
        self.lines
            .push_span(Span::styled(text.to_string(), style), true);
    }

    fn push_code_span(&mut self, text: CowStr<'_>) {
        let style = self
            .current_style()
            .fg(Color::Yellow)
            .add_modifier(Modifier::DIM);
        if self.lines.is_line_start() {
            self.insert_prefixes();
        }
        self.lines
            .push_span(Span::styled(format!("`{}`", text), style), true);
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

    fn push_table_html(&mut self, html: &CowStr<'_>) -> bool {
        if let Some(table) = self.table.as_mut() {
            if table.is_collecting() {
                table.push_html(html);
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
        if self.code_block.is_active() {
            self.lines
                .push_span(Span::styled("    ", Self::code_block_style()), false);
        }
        if self.blockquote_depth > 0 {
            let mut prefix = String::new();
            for _ in 0..self.blockquote_depth {
                prefix.push_str("> ");
            }
            let mut style = Style::default().fg(Color::DarkGray);
            if self.code_block.is_active() {
                style = style.bg(CODE_BLOCK_BG);
            }
            self.lines.push_span(Span::styled(prefix, style), false);
        }
    }

    fn finish_code_block(&mut self) {
        let Some((start, language)) = self.code_block.take() else {
            return;
        };
        if start > self.lines.len() {
            return;
        }
        if start == self.lines.len() {
            self.lines.push_manual_line(Line::default());
        }
        let end = self.lines.len();
        if end > start {
            self.code_blocks.push(CodeBlockOverlay {
                line_start: start,
                line_end: end,
                language,
            });
        }
    }

    fn soft_break(&mut self) {
        self.flush_line(false);
    }

    fn hard_break(&mut self) {
        self.flush_line(true);
    }

    fn flush_line(&mut self, allow_empty: bool) {
        self.lines.flush_line(allow_empty);
    }
    fn ensure_block_gap(&mut self) {
        self.lines.ensure_block_gap();
    }

    fn push_blank_line(&mut self) {
        self.lines.push_blank_line();
    }

    fn push_rule(&mut self) {
        self.ensure_block_gap();
        let line_index = self.lines.push_manual_line(Line::default());
        self.rule_lines.push(line_index);
        self.lines.mark_last_non_blank();
        self.lines.push_blank_line();
    }

    fn finalize(self) -> RenderedMarkdown {
        let (lines, headings) = self.lines.finalize();
        RenderedMarkdown {
            lines,
            headings,
            code_blocks: self.code_blocks,
            rules: self.rule_lines,
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

fn unordered_bullet(depth: usize) -> &'static str {
    const BULLETS: [&str; 4] = ["●", "○", "■", "□"];
    BULLETS[depth % BULLETS.len()]
}

struct TableBuilder {
    alignments: Vec<Alignment>,
    header: Option<Vec<Cell>>,
    rows: Vec<Vec<Cell>>,
    current_row: Vec<Cell>,
    current_cell: String,
    in_head: bool,
    in_cell: bool,
    max_width: usize,
}

impl TableBuilder {
    fn new(alignments: Vec<Alignment>, max_width: usize) -> Self {
        Self {
            alignments,
            header: None,
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            in_head: false,
            in_cell: false,
            max_width,
        }
    }

    fn start_head(&mut self) {
        if self.in_cell {
            self.end_cell();
        }
        self.current_row.clear();
        self.in_head = true;
    }

    fn end_head(&mut self) {
        if self.in_cell {
            self.end_cell();
        }
        if self.in_head {
            self.commit_header_row();
        }
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
        if self.in_head {
            self.commit_header_row();
        } else {
            self.rows.push(self.current_row.clone());
            self.current_row.clear();
        }
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
        let raw = mem::take(&mut self.current_cell);
        let cell = Cell::from_raw(raw);
        self.current_row.push(cell);
        self.in_cell = false;
    }

    fn commit_header_row(&mut self) {
        if self.current_row.is_empty() {
            return;
        }
        if self.header.is_none() {
            self.header = Some(self.current_row.clone());
        }
        self.current_row.clear();
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
        self.current_cell.push('\n');
    }

    fn push_html(&mut self, html: &CowStr<'_>) {
        if !self.in_cell {
            return;
        }
        if is_html_break(html.as_ref()) {
            self.current_cell.push('\n');
        } else {
            self.current_cell.push_str(html.as_ref());
        }
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
        clamp_column_widths(&mut widths, self.max_width);

        let mut lines = Vec::new();
        lines.push(Line::from(unicode_border('┌', '┬', '┐', &widths)));
        if let Some(header) = &self.header {
            for row_line in build_row_lines(header, &widths, &self.alignments) {
                lines.push(Line::from(row_line));
            }
            lines.push(Line::from(unicode_border('├', '┼', '┤', &widths)));
        }
        for (idx, row) in self.rows.iter().enumerate() {
            for row_line in build_row_lines(row, &widths, &self.alignments) {
                lines.push(Line::from(row_line));
            }
            if idx + 1 < self.rows.len() {
                lines.push(Line::from(unicode_border('├', '┼', '┤', &widths)));
            }
        }
        lines.push(Line::from(unicode_border('└', '┴', '┘', &widths)));
        lines
    }
}

#[derive(Clone)]
struct Cell {
    lines: Vec<String>,
}

impl Cell {
    fn from_raw(raw: String) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self {
                lines: vec![String::new()],
            };
        }
        let mut lines: Vec<String> = trimmed
            .split('\n')
            .map(|segment| segment.trim().to_string())
            .collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self { lines }
    }

    fn width(&self) -> usize {
        self.lines
            .iter()
            .map(|line| display_width(line))
            .max()
            .unwrap_or(0)
    }
}

fn update_widths(widths: &mut [usize], row: &[Cell]) {
    for (idx, cell) in row.iter().enumerate() {
        if idx < widths.len() {
            widths[idx] = widths[idx].max(cell.width());
        }
    }
}

fn display_width(value: &str) -> usize {
    let width = UnicodeWidthStr::width(value);
    width.max(1)
}

fn build_row_lines(row: &[Cell], widths: &[usize], alignments: &[Alignment]) -> Vec<String> {
    if widths.is_empty() {
        return Vec::new();
    }
    let mut column_lines: Vec<Vec<String>> = widths
        .iter()
        .enumerate()
        .map(|(idx, width)| render_cell_lines(row.get(idx), *width, alignments[idx]))
        .collect();
    let height = column_lines
        .iter()
        .map(|lines| lines.len())
        .max()
        .unwrap_or(1);
    for (col_idx, lines) in column_lines.iter_mut().enumerate() {
        while lines.len() < height {
            lines.push(pad_cell("", widths[col_idx], alignments[col_idx]));
        }
    }
    let mut rows = Vec::with_capacity(height);
    for line_idx in 0..height {
        let mut line = String::new();
        line.push('│');
        for col_idx in 0..widths.len() {
            line.push(' ');
            line.push_str(&column_lines[col_idx][line_idx]);
            line.push(' ');
            line.push('│');
        }
        rows.push(line);
    }
    rows
}

fn pad_cell(text: &str, width: usize, alignment: Alignment) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return " ".repeat(width);
    }
    let display = UnicodeWidthStr::width(trimmed);
    if display >= width {
        return trimmed.to_string();
    }
    let padding = width - display;
    match alignment {
        Alignment::Right => format!("{}{}", " ".repeat(padding), trimmed),
        Alignment::Center => {
            let left = padding / 2;
            let right = padding - left;
            format!("{}{}{}", " ".repeat(left), trimmed, " ".repeat(right))
        }
        _ => format!("{}{}", trimmed, " ".repeat(padding)),
    }
}

fn unicode_border(left: char, junction: char, right: char, widths: &[usize]) -> String {
    let mut line = String::new();
    line.push(left);
    for (idx, width) in widths.iter().enumerate() {
        line.push_str(&"─".repeat(width + 2));
        if idx + 1 == widths.len() {
            line.push(right);
        } else {
            line.push(junction);
        }
    }
    line
}

fn is_html_break(value: &str) -> bool {
    let lowered = value.trim().to_ascii_lowercase();
    matches!(lowered.as_str(), "<br>" | "<br/>" | "<br />")
}

fn clamp_column_widths(widths: &mut [usize], max_width: usize) {
    if widths.is_empty() {
        return;
    }
    let border_space = 3 * widths.len() + 1;
    if border_space >= max_width {
        widths.fill(MIN_COLUMN_WIDTH);
        return;
    }
    let max_content = max_width.saturating_sub(border_space);
    let min_total = MIN_COLUMN_WIDTH * widths.len();
    if max_content <= min_total {
        widths.fill(MIN_COLUMN_WIDTH);
        return;
    }
    let total: usize = widths.iter().sum();
    if total <= max_content {
        return;
    }
    let scale = max_content as f64 / total as f64;
    for width in widths.iter_mut() {
        let scaled = (*width as f64 * scale).floor() as usize;
        *width = scaled.max(MIN_COLUMN_WIDTH);
    }
    adjust_widths(widths, max_content);
}

fn adjust_widths(widths: &mut [usize], target: usize) {
    if widths.is_empty() {
        return;
    }
    let mut total: isize = widths.iter().sum::<usize>() as isize;
    let target = target as isize;
    if total > target {
        while total > target {
            if let Some((idx, _)) = widths
                .iter()
                .enumerate()
                .filter(|(_, &w)| w > MIN_COLUMN_WIDTH)
                .max_by_key(|(_, &w)| w)
            {
                widths[idx] -= 1;
                total -= 1;
            } else {
                break;
            }
        }
    } else if total < target {
        let mut idx = 0usize;
        while total < target {
            widths[idx % widths.len()] += 1;
            total += 1;
            idx += 1;
        }
    }
}

fn render_cell_lines(cell: Option<&Cell>, width: usize, alignment: Alignment) -> Vec<String> {
    let mut rendered = Vec::new();
    if let Some(cell) = cell {
        for raw_line in &cell.lines {
            let wrapped = wrap_cell_text(raw_line, width);
            if wrapped.is_empty() {
                rendered.push(pad_cell("", width, alignment));
            } else {
                for segment in wrapped {
                    rendered.push(pad_cell(&segment, width, alignment));
                }
            }
        }
    }
    if rendered.is_empty() {
        rendered.push(pad_cell("", width, alignment));
    }
    rendered
}

fn wrap_cell_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![String::new()];
    }
    wrap(trimmed, WrapOptions::new(width).break_words(true))
        .into_iter()
        .map(|segment| segment.into_owned())
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

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
    fn table_builder_outputs_unicode_rows() {
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
        assert!(joined.contains("┌"));
        assert!(joined.contains("│ x"));
    }

    #[test]
    fn table_headers_are_rendered() {
        let markdown = "| Name | Age |\n| --- | --- |\n| Bob | 3 |";
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
        assert!(joined.contains("Name"));
        assert!(joined.contains("Age"));
    }

    #[test]
    fn table_cells_render_multiline_content() {
        let markdown = "| A |\n| --- |\n| line1<br>line2 |";
        let render = markdown_to_render(markdown);
        let joined = render
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
        let lines: Vec<_> = joined.lines().collect();
        let line1_idx = lines.iter().position(|line| line.contains("line1"));
        let line2_idx = lines.iter().position(|line| line.contains("line2"));
        assert!(line1_idx.is_some());
        assert!(line2_idx.is_some());
        assert_ne!(line1_idx.unwrap(), line2_idx.unwrap());
    }

    #[test]
    fn table_handles_wide_characters() {
        let markdown = "| Emoji | Word |\n| --- | --- |\n| 😀 | text |";
        let render = markdown_to_render(markdown);
        let joined: Vec<String> = render
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        let border = joined
            .iter()
            .find(|line| line.starts_with('┌'))
            .expect("table border present");
        let emoji_line = joined
            .iter()
            .find(|line| line.contains('😀'))
            .expect("emoji row present");
        assert_eq!(
            UnicodeWidthStr::width(border.as_str()),
            UnicodeWidthStr::width(emoji_line.as_str())
        );
    }

    #[test]
    fn wide_tables_wrap_cells() {
        let markdown = "| A | B | C | D | E | F |\n| --- | --- | --- | --- | --- | --- |\n| superlongwordwithoutbreaksandmoresuperlongwordwithoutbreaksandmoresuperlongwordwithoutbreaksandmore | content | content | content | content | content |";
        let render = markdown_to_render(markdown);
        let joined: Vec<String> = render
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        let total_row_lines = joined.iter().filter(|line| line.starts_with('│')).count();
        assert!(
            total_row_lines >= 3,
            "header plus wrapped row should yield multiple row lines"
        );
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
    fn unordered_lists_render_nested_bullets() {
        let markdown = "- top\n  - child\n    - grandchild";
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
        assert!(text.contains("● top"));
        assert!(text.contains("  ○ child"));
        assert!(text.contains("    ■ grandchild"));
    }

    #[test]
    fn rule_lines_are_recorded() {
        let markdown = "before\n\n---\n\nafter";
        let render = markdown_to_render(markdown);
        assert_eq!(render.rules.len(), 1);
        let line_idx = render.rules[0];
        assert!(line_idx < render.lines.len());
        assert!(render.lines[line_idx].spans.is_empty());
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
