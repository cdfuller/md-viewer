# Markdown Viewer Demo

Welcome to **md-viewer**! This sample file shows a range of supported features so you can quickly verify rendering.

---

## Lists

- Plain bullet items
- Nested list:
  - child item one
  - child item two
- Task list:
  - [x] done item
  - [ ] unfinished item

1. Ordered entry one
2. Ordered entry two

## Block Quote

> Ratatui makes it easy to build terminal user interfaces.
> 
> > Quotes can nest as well.

## Inline Styles

You can mix _italics_, **bold**, and ~~strikethrough~~, plus inline code like `let x = 42;` and [links](https://ratatui.rs).

## Code Block

```rust
use std::time::{Duration, Instant};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

struct Stopwatch {
    started_at: Instant,
}

impl Stopwatch {
    fn start() -> Self {
        Self { started_at: Instant::now() }
    }

    fn elapsed_ms(&self) -> u128 {
        self.started_at.elapsed().as_millis()
    }
}

fn main() -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let stopwatch = Stopwatch::start();
    terminal.draw(|frame| {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)])
            .split(frame.size());

        let elapsed = format!("{} ms elapsed", stopwatch.elapsed_ms());
        frame.render_widget(
            Paragraph::new(elapsed).block(Block::default().title("Stopwatch").borders(Borders::ALL)),
            layout[0],
        );
    })?;

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
    Ok(())
}
```

## Footnotes & Tables

Here is a footnote reference.[^1]

| Feature   | Status | Notes                  |
|-----------|--------|-----------------------|
| Tables    | ✅     | Column widths auto-fit |
| Footnotes | ✅     | Styled references      |
| Reload    | ✅     | Press `r`              |

[^1]: Footnote definitions appear near the bottom.

## Extra Paragraphs

This section pads the file so scrolling can be tested thoroughly. Duplicate paragraphs, varying sentence lengths, and occasional inline styles help ensure wrapping and spacing feel natural. Keep pressing PgDown to push the viewport; use PgUp and `g`/`G` to jump around quickly.

Another paragraph keeps going with more descriptive text. Maybe talk about your favorite Markdown extensions, or how Ratatui widgets let you build rich TUIs without touching lower-level terminal escape sequences. The goal is simply to have enough content to exercise scrolling, re-rendering after reloads, and high-contrast text on the status bar.

Final paragraph! Save this file, run `cargo run -- example.md`, and enjoy the viewer.

## Extended Guide

Below is an extra set of sections that mirror real-world documentation. Having roughly twice as much content helps validate smooth scrolling, performance, and table rendering even for longer files.

### Usage Tips

- Launch the viewer in one terminal and keep a text editor open in another.
- Make edits, save, and press `r` in the viewer to reload instantly.
- Toggle between files by quitting (`q`), then running `cargo run -- <file>` again.

When rendering very long bullet lists, ensure that the indentation feels consistent. Eight or ten nested levels can get tricky, so try it below:

- root level
  - level two
    - level three
      - level four
        - level five
          - level six
            - level seven
              - level eight
                - level nine
                  - level ten

### Second Code Sample

```rust
#[derive(Debug)]
enum Command {
    Quit,
    Reload,
    Scroll { lines: i32 },
}

impl Command {
    fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("quit") {
            return Some(Self::Quit);
        }
        if input.eq_ignore_ascii_case("reload") {
            return Some(Self::Reload);
        }
        if let Some(rest) = input.strip_prefix("scroll ") {
            if let Ok(lines) = rest.parse() {
                return Some(Self::Scroll { lines });
            }
        }
        None
    }
}

fn run_command(cmd: Command) {
    match cmd {
        Command::Quit => println!("Goodbye"),
        Command::Reload => println!("Reloading file"),
        Command::Scroll { lines } => println!("Scrolling {lines} lines"),
    }
}

fn main() {
    for sample in ["quit", "reload", "scroll 10", "scroll -5"] {
        if let Some(cmd) = Command::parse(sample) {
            run_command(cmd);
        }
    }
}
```

### Large Table

| Module       | Purpose                     | Notes                       | Status |
|--------------|-----------------------------|-----------------------------|--------|
| App          | Handles scrolling & reloads | Stores state for the viewer | Ready  |
| Markdown     | Parses Markdown to spans    | Uses pulldown-cmark         | Ready  |
| TableBuilder | Formats aligned tables      | New feature                 | Beta   |
| UI           | Draws Ratatui widgets       | Layout + status bar         | Ready  |

### More Paragraphs

To thoroughly test rendering, you need walls of text. Here is one: Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum sed augue tempor, porttitor mauris ac, auctor mauris. Integer pulvinar mi eu nisi molestie, id sollicitudin tortor convallis. Sed fringilla lectus eu augue dictum, nec placerat nisi lobortis. Curabitur rhoncus, metus id hendrerit blandit, diam lacus varius justo, quis interdum sapien sem eu libero.

Another block of filler ensures wrap behavior gets plenty of coverage. Suspendisse suscipit ligula ut turpis rhoncus, vel gravida lectus elementum. Donec pharetra interdum interdum. Morbi facilisis dui sed tristique laoreet. Nullam ultricies turpis nec lorem rutrum, sit amet auctor nisl volutpat. Integer vel mi quis augue fermentum porta non id nibh.

Yet another paragraph, now with inline references to `code`, **bold statements**, and _italic emphasis_. Try resizing the terminal window while this file is open; the viewer should continue to reflow content gracefully.

### Closing Thoughts

Thanks for putting md-viewer through its paces. The more content you have here, the easier it is to experiment with new rendering ideas like syntax highlighting, inline images, or even interactive table sorting in the future. Happy hacking!
