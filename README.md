# md-viewer

> Terminal markdown viewer built with Rust and Ratatui.

md-viewer renders CommonMark files directly in your terminal using Ratatui's rich text widgets. It highlights headings, block quotes, lists, inline code, and fenced code blocks while giving you smooth scrolling controls. The viewer reloads content on demand so you can keep editing in another window and refresh with a single keypress.

## Getting Started

1. Install the Rust toolchain (https://rustup.rs/) so `cargo` is available on your PATH.
2. Fetch dependencies and build the project:
   ```sh
   cargo build
   ```
3. Run the viewer against any markdown file:
   ```sh
   cargo run -- path/to/file.md
   ```
4. (Optional) Run formatter and tests:
   ```sh
   cargo fmt
   cargo test
   ```

## Controls

- `n` / `p`: scroll one line (arrows still work too)
- `Space` / `Shift+Space`: scroll by one viewport
- `PgUp` / `PgDn`: also scroll by a viewport
- `g` / `Home`: jump to the top
- `G` / `End`: jump to the bottom
- `r`: reload the file from disk
- `q` or `Ctrl+C`: exit the application

## Development Notes

- The renderer is powered by `pulldown-cmark` so most CommonMark features (tables, task lists, footnotes, etc.) display with sensible terminal-friendly styling.
- Rendering happens on every draw call; large files benefit from release builds (`cargo run --release`).
- The status bar at the bottom shows key bindings and the latest status message (reload success/failure, etc.).
