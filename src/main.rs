use ratatui::{
    crossterm::{
        event::{self, Event, KeyEventKind, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    layout::{Constraint, Layout},
    prelude::*,
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::io::{self, stdout};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = setup_terminal()?;
    let result = run_app(&mut terminal);
    restore_terminal()?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>, io::Error> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    Terminal::new(backend)
}

fn restore_terminal() -> Result<(), io::Error> {
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('j') | KeyCode::Down => app.select_next(),
                    KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
    ]).split(area);

    // Header
    let header = Paragraph::new("ehentai-tui - e-hentai.org manga viewer")
        .style(Style::default().fg(Color::Cyan).bold())
        .block(Block::default().borders(Borders::ALL).title("ehentai-tui"));
    f.render_widget(header, chunks[0]);

    // Body - Manga list
    let items: Vec<ListItem> = app.items
        .iter()
        .map(|item| ListItem::new(Line::from(item.clone())))
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Manga Library"))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(Some(app.selected));
    f.render_stateful_widget(list, chunks[1], &mut state);
}

struct App {
    items: Vec<String>,
    selected: usize,
}

impl App {
    fn select_next(&mut self) {
        if !self.items.is_empty() && self.selected < self.items.len() - 1 {
            self.selected += 1;
        }
    }

    fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
}

impl App {
    fn new() -> Self {
        Self {
            items: vec![
                "Manga 1 - Title".to_string(),
                "Manga 2 - Title".to_string(),
                "Manga 3 - Title".to_string(),
            ],
            selected: 0,
        }
    }
}