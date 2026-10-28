# ehentai-tui

A terminal user interface (TUI) for browsing and viewing [e-hentai.org](https://e-hentai.org) manga, built with Rust and [ratatui](https://github.com/ratatui-org/ratatui).

## Features

- Browse and search e-hentai.org galleries from the terminal
- View manga pages with keyboard navigation
- Lightweight and fast

## Requirements

- Rust 1.85+ (edition 2024)
- A terminal that supports alternate screen mode

## Building

```sh
git clone https://github.com/mnk-works/ehentai-tui.git
cd ehentai-tui
cargo build --release
```

## Usage

```sh
cargo run
```

| Key | Action |
|-----|--------|
| `q` | Quit |
| `j` / `↓` | Select next item |
| `k` / `↑` | Select previous item |

## License

Dual-licensed under MIT or Apache-2.0, at your option.
