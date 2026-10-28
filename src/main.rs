use clap::{Parser, Subcommand};
use ratatui::{
    crossterm::{
        event::{self, Event, KeyEventKind, KeyCode, KeyModifiers},
        ExecutableCommand,
    },
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT, REFERER};
use scraper::{Html, Selector};
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::io::{self, stdout};
use std::time::{Duration, Instant};
use urlencoding::encode;

// ==================== CLI ====================

#[derive(Parser, Debug)]
#[command(name = "ehentai", version = "0.1.0", about = "Terminal viewer for e-hentai.org manga")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Search e-hentai.org for galleries
    Search {
        /// Search keyword
        query: String,
        /// Page number
        #[arg(short, long, default_value = "1")]
        page: u32,
    },
    /// View a specific gallery
    View {
        /// Gallery ID
        gallery_id: u64,
        /// Gallery token
        #[arg(short, long)]
        token: Option<String>,
    },
}

// ==================== DATA MODELS ====================

#[derive(Debug, Clone)]
struct RequestLog {
    pub method: String,
    pub url: String,
    pub status: String,
    pub status_code: Option<u16>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
struct Gallery {
    pub id: u64,
    pub token: String,
    pub title: String,
    pub page_count: usize,
    pub uploader: String,
    pub posted: String,
    pub url: String,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone)]
struct GalleryPage {
    pub index: usize,
    pub filename: String,
    pub image_url: String,
    pub size: Option<u64>,
}

// ==================== HTTP CLIENT ====================

struct EhentaiClient {
    client: Client,
    logs: Arc<Mutex<VecDeque<RequestLog>>>,
}

impl EhentaiClient {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36"));
        headers.insert(REFERER, HeaderValue::from_static("https://e-hentai.org/"));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(100))),
        }
    }

    pub fn pop_log(&self) -> Option<RequestLog> {
        let mut logs = self.logs.lock().unwrap();
        logs.pop_front()
    }

    fn get(&self, url: &str) -> Result<String, Box<dyn std::error::Error>> {
        let start = Instant::now();
        let response = self.client.get(url).send()?;
        let duration = start.elapsed();
        let status_code = response.status().as_u16();
        let status_text = response.status().canonical_reason().unwrap_or("Unknown").to_string();
        let text = response.text()?;

        let log = RequestLog {
            method: "GET".to_string(),
            url: url.to_string(),
            status: format!("{} {}", status_code, status_text),
            status_code: Some(status_code),
            duration_ms: duration.as_millis() as u64,
        };

        let mut logs = self.logs.lock().unwrap();
        if logs.len() >= 100 {
            logs.pop_front();
        }
        logs.push_back(log);

        Ok(text)
    }

    pub fn search(&self, query: &str, page: u32) -> Result<Vec<Gallery>, Box<dyn std::error::Error>> {
        let encoded = encode(query);
        let url = format!("https://e-hentai.org/?page={}&keyword={}", page - 1, encoded);
        let html = self.get(&url)?;
        let document = Html::parse_document(&html);

        let row_selector = Selector::parse("table.itg tr").unwrap();
        let link_selector = Selector::parse("td.gl3c.glname a").unwrap();
        let title_selector = Selector::parse(".glink").unwrap();
        let thumb_selector = Selector::parse(".glthumb img[data-src]").unwrap();
        let pages_selector = Selector::parse("td.gl4c.glhide div:nth-child(2)").unwrap();
        let uploader_selector = Selector::parse("td.gl4c.glhide a").unwrap();
        let posted_selector = Selector::parse("td.gl2c div[id^='posted_']").unwrap();

        let mut galleries = Vec::new();

        for row in document.select(&row_selector) {
            let link = row.select(&link_selector).next();
            let href = link.and_then(|l| l.value().attr("href"));

            if let Some(href) = href {
                let parts: Vec<&str> = href.split('/').filter(|s| !s.is_empty()).collect();
                if parts.len() >= 3 && parts[0] == "g" {
                    let id = parts[1].parse::<u64>().unwrap_or(0);
                    let token = parts[2].to_string();
                    if id > 0 {
                        let title = row.select(&title_selector)
                            .next()
                            .map(|t| t.text().collect::<String>().trim().to_string())
                            .unwrap_or_else(|| "Unknown".to_string());

                        let thumbnail = row.select(&thumb_selector)
                            .next()
                            .and_then(|img| img.value().attr("data-src"))
                            .map(|s| s.to_string());

                        let page_count = row.select(&pages_selector)
                            .next()
                            .and_then(|d| d.text().next())
                            .and_then(|s| s.trim().split_whitespace().next())
                            .and_then(|n| n.parse::<usize>().ok())
                            .unwrap_or(0);

                        let uploader = row.select(&uploader_selector)
                            .next()
                            .map(|a| a.text().collect::<String>().trim().to_string())
                            .unwrap_or_else(|| "Unknown".to_string());

                        let posted = row.select(&posted_selector)
                            .next()
                            .map(|d| d.text().collect::<String>().trim().to_string())
                            .unwrap_or_else(|| "Unknown".to_string());

                        galleries.push(Gallery {
                            id,
                            token,
                            title,
                            page_count,
                            uploader,
                            posted,
                            url: format!("https://e-hentai.org{}", href),
                            thumbnail_url: thumbnail,
                        });
                    }
                }
            }
        }

        Ok(galleries)
    }

    pub fn get_gallery_pages(&self, gid: &u64, token: &str) -> Result<Vec<GalleryPage>, Box<dyn std::error::Error>> {
        let url = format!("https://e-hentai.org/g/{}/{}", gid, token);
        let html = self.get(&url)?;
        let document = Html::parse_document(&html);

        let page_selector = Selector::parse("#gdt a").unwrap();
        let page_links: Vec<_> = document.select(&page_selector).collect();

        let mut pages = Vec::new();

        for (idx, link) in page_links.iter().enumerate() {
            if let Some(href) = link.value().attr("href") {
                let full_url = if href.starts_with("http") {
                    href.to_string()
                } else {
                    format!("https://e-hentai.org{}", href)
                };

                let title = link.value().attr("title").unwrap_or("").to_string();
                let filename = if !title.is_empty() {
                    title
                } else {
                    format!("page_{}.jpg", idx + 1)
                };

                pages.push(GalleryPage {
                    index: idx,
                    filename,
                    image_url: full_url,
                    size: None,
                });
            }
        }

        if pages.is_empty() {
            let sprite_selector = Selector::parse("#gdt div[style*='background']").unwrap();
            for (idx, div) in document.select(&sprite_selector).enumerate() {
                if let Some(style) = div.value().attr("style") {
                    if let Some(url_start) = style.find("url(") {
                        let url_end = style[url_start + 4..].find(')');
                        if let Some(url_end) = url_end {
                            let bg_url = &style[url_start + 4..url_end].trim().trim_matches('"').trim_matches('\'');
                            pages.push(GalleryPage {
                                index: idx,
                                filename: format!("page_{}.webp", idx + 1),
                                image_url: bg_url.to_string(),
                                size: None,
                            });
                        }
                    }
                }
            }
        }

        Ok(pages)
    }
}

// ==================== TUI ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Search,
    GalleryList,
    GalleryDetail,
    PageView,
}

struct LogEntry {
    timestamp: String,
    method: String,
    url: String,
    status: String,
    duration_ms: u64,
    is_error: bool,
}

impl LogEntry {
    fn from_log(log: &RequestLog) -> Self {
        let is_error = log.status_code.unwrap_or(200) >= 400;
        Self {
            timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
            method: log.method.clone(),
            url: log.url.clone(),
            status: log.status.clone(),
            duration_ms: log.duration_ms,
            is_error,
        }
    }
}

struct App {
    mode: AppMode,
    client: EhentaiClient,
    search_query: String,
    search_cursor: usize,
    gallery_list_selected: usize,
    gallery_list_state: ListState,
    galleries: Vec<Gallery>,
    gallery_pages: Vec<GalleryPage>,
    page_selected: usize,
    page_state: ListState,
    current_page: Option<GalleryPage>,
    logs: Vec<LogEntry>,
    log_scroll: usize,
    status_message: String,
    last_error: Option<String>,
    selected_gallery: Option<Gallery>,
}

impl App {
    fn new() -> Self {
        let mut gallery_list_state = ListState::default();
        gallery_list_state.select(Some(0));
        let mut page_state = ListState::default();
        page_state.select(Some(0));

        Self {
            mode: AppMode::Search,
            client: EhentaiClient::new(),
            search_query: "anime".to_string(),
            search_cursor: 5,
            gallery_list_selected: 0,
            gallery_list_state,
            galleries: Vec::new(),
            gallery_pages: Vec::new(),
            page_selected: 0,
            page_state,
            current_page: None,
            logs: Vec::new(),
            log_scroll: 0,
            status_message: "Press 's' to search, 'q' to quit".to_string(),
            last_error: None,
            selected_gallery: None,
        }
    }

    fn sync_logs(&mut self) {
        while let Some(log) = self.client.pop_log() {
            self.logs.push(LogEntry::from_log(&log));
            if self.logs.len() > 50 {
                self.logs.remove(0);
            }
        }
    }

    fn run_search(&mut self, query: String) {
        self.mode = AppMode::GalleryList;
        self.status_message = format!("Searching for '{}'...", query);
        self.sync_logs();

        match self.client.search(&query, 1) {
            Ok(galleries) => {
                self.galleries = galleries;
                self.gallery_list_selected = 0;
                self.gallery_list_state.select(Some(0));
                self.status_message = format!("Found {} galleries", self.galleries.len());
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
                self.status_message = "Search failed".to_string();
            }
        }
        self.sync_logs();
    }

    fn open_gallery(&mut self, idx: usize) {
        if idx >= self.galleries.len() {
            return;
        }
        let gallery = self.galleries[idx].clone();
        self.selected_gallery = Some(gallery.clone());
        self.status_message = format!("Loading gallery: {}...", gallery.title);
        self.sync_logs();

        match self.client.get_gallery_pages(&gallery.id, &gallery.token) {
            Ok(pages) => {
                self.gallery_pages = pages;
                self.page_selected = 0;
                self.page_state.select(Some(0));
                self.mode = AppMode::GalleryDetail;
                self.status_message = format!("Gallery '{}' - {} pages", gallery.title, self.gallery_pages.len());
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
                self.status_message = "Failed to load gallery".to_string();
            }
        }
        self.sync_logs();
    }

    fn open_page(&mut self, idx: usize) {
        if idx >= self.gallery_pages.len() {
            return;
        }
        let page = self.gallery_pages[idx].clone();
        self.current_page = Some(page.clone());
        self.status_message = format!("Page {}: {}...", idx + 1, page.filename);
        self.sync_logs();

        self.mode = AppMode::PageView;
        self.status_message = format!("Page {}: {} | {}", idx + 1, page.filename, page.image_url);
        self.sync_logs();
    }

    fn go_back(&mut self) {
        match self.mode {
            AppMode::PageView => {
                self.mode = AppMode::GalleryDetail;
                self.current_page = None;
                self.status_message = format!("Gallery '{}' - {} pages", 
                    self.selected_gallery.as_ref().map(|g| g.title.as_str()).unwrap_or(""),
                    self.gallery_pages.len());
            }
            AppMode::GalleryDetail => {
                self.mode = AppMode::GalleryList;
                self.selected_gallery = None;
                self.gallery_pages.clear();
                self.status_message = format!("Found {} galleries", self.galleries.len());
            }
            AppMode::GalleryList => {
                self.mode = AppMode::Search;
                self.galleries.clear();
                self.status_message = "Search mode. Press 's' to search, 'q' to quit".to_string();
            }
            AppMode::Search => {}
        }
        self.sync_logs();
    }

    fn scroll_logs(&mut self, delta: isize) {
        let max_scroll = self.logs.len().saturating_sub(8);
        self.log_scroll = (self.log_scroll as isize + delta).clamp(0, max_scroll as isize) as usize;
    }

    fn handle_search_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char(c) => {
                self.search_query.insert(self.search_cursor, c);
                self.search_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                    self.search_query.remove(self.search_cursor);
                }
            }
            KeyCode::Left => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.search_cursor < self.search_query.len() {
                    self.search_cursor += 1;
                }
            }
            KeyCode::Enter => {
                let query = self.search_query.clone();
                self.run_search(query);
            }
            KeyCode::Esc => self.mode = AppMode::GalleryList,
            _ => {}
        }
    }

    fn select_next_gallery(&mut self) {
        if !self.galleries.is_empty() && self.gallery_list_selected < self.galleries.len() - 1 {
            self.gallery_list_selected += 1;
            self.gallery_list_state.select(Some(self.gallery_list_selected));
        }
    }

    fn select_prev_gallery(&mut self) {
        if self.gallery_list_selected > 0 {
            self.gallery_list_selected -= 1;
            self.gallery_list_state.select(Some(self.gallery_list_selected));
        }
    }

    fn select_next_page(&mut self) {
        if !self.gallery_pages.is_empty() && self.page_selected < self.gallery_pages.len() - 1 {
            self.page_selected += 1;
            self.page_state.select(Some(self.page_selected));
        }
    }

    fn select_prev_page(&mut self) {
        if self.page_selected > 0 {
            self.page_selected -= 1;
            self.page_state.select(Some(self.page_selected));
        }
    }
}

// ==================== MAIN ====================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Handle CLI commands
    if let Some(commands) = cli.command {
        let client = EhentaiClient::new();
        match commands {
            Commands::Search { query, page } => {
                println!("Searching e-hentai.org for '{}'...", query);
                match client.search(&query, page) {
                    Ok(galleries) => {
                        println!();
                        for g in &galleries {
                            println!("{:<10} {:<8} {}", g.id, g.page_count, g.title);
                            println!("{:<10} {:<8} {}", g.id, g.page_count, g.url);
                        }
                        println!();
                        println!("Found {} galleries", galleries.len());
                    }
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
            Commands::View { gallery_id, token } => {
                println!("Fetching gallery {}...", gallery_id);
                let token = token.unwrap_or_else(|| "dummy".to_string());
                match client.get_gallery_pages(&gallery_id, &token) {
                    Ok(pages) => {
                        println!();
                        println!("Gallery {}: {} pages", gallery_id, pages.len());
                        println!();
                        for p in &pages {
                            println!("{} | {}", p.filename, p.image_url);
                        }
                        println!();
                        println!("Total pages: {}", pages.len());
                    }
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
        }
    }

    // TUI mode
    run_tui()
}

fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = setup_terminal()?;
    let mut app = App::new();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match app.mode {
                    AppMode::Search => {
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                // This would normally trigger a search
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                            _ => app.handle_search_input(key.code),
                        }
                    }
                    AppMode::GalleryList => {
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('j') | KeyCode::Down => app.select_next_gallery(),
                            KeyCode::Char('k') | KeyCode::Up => app.select_prev_gallery(),
                            KeyCode::Enter => app.open_gallery(app.gallery_list_selected),
                            KeyCode::Char('r') => {
                                if let Some(_g) = app.galleries.get(app.gallery_list_selected) {
                                    app.open_gallery(app.gallery_list_selected);
                                }
                            }
                            KeyCode::Char('l') => app.scroll_logs(1),
                            KeyCode::Char('h') => app.scroll_logs(-1),
                            _ => {}
                        }
                    }
                    AppMode::GalleryDetail => {
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('j') | KeyCode::Down => app.select_next_page(),
                            KeyCode::Char('k') | KeyCode::Up => app.select_prev_page(),
                            KeyCode::Enter => app.open_page(app.page_selected),
                            KeyCode::Char('b') | KeyCode::Esc => app.go_back(),
                            KeyCode::Char('l') => app.scroll_logs(1),
                            KeyCode::Char('h') => app.scroll_logs(-1),
                            _ => {}
                        }
                    }
                    AppMode::PageView => {
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('b') | KeyCode::Esc => app.go_back(),
                            KeyCode::Char('l') => app.scroll_logs(1),
                            KeyCode::Char('h') => app.scroll_logs(-1),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    restore_terminal()?;
    Ok(())
}

fn setup_terminal() -> Result<ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>, io::Error> {
    crossterm::terminal::enable_raw_mode()?;
    stdout().execute(crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    Ok(ratatui::Terminal::new(backend)?)
}

fn restore_terminal() -> Result<(), io::Error> {
    crossterm::terminal::disable_raw_mode()?;
    stdout().execute(crossterm::terminal::LeaveAlternateScreen)?;
    Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &mut App) {
    app.sync_logs();

    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(1),
        Constraint::Length(8),
    ]).split(area);

    render_header(f, app, chunks[0]);
    render_content(f, app, chunks[1]);
    render_status(f, app, chunks[2]);
    render_log_bar(f, app, chunks[3]);
}

fn render_header(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let title = match app.mode {
        AppMode::GalleryList => "ehentai-tui - Gallery List".to_string(),
        AppMode::GalleryDetail => {
            if let Some(g) = &app.selected_gallery {
                format!("ehentai-tui - {}", g.title)
            } else {
                "ehentai-tui - Gallery Detail".to_string()
            }
        }
        AppMode::PageView => {
            if let Some(p) = &app.current_page {
                format!("ehentai-tui - Page: {}", p.filename)
            } else {
                "ehentai-tui - Page View".to_string()
            }
        }
        AppMode::Search => "ehentai-tui - Search".to_string(),
    };

    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).bold())
        .block(Block::default().borders(Borders::ALL).title("ehentai-tui"));
    f.render_widget(header, area);
}

fn render_content(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    match app.mode {
        AppMode::Search => {
            let help = Paragraph::new(
                "Type search query, press Enter to search, Esc to cancel\n\
                Use j/k or arrows to navigate when results appear\n\
                Press 's' in gallery list to return to search"
            )
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL).title("Search Help"))
            .wrap(Wrap { trim: true });
            f.render_widget(help, area);
        }
        AppMode::GalleryList => render_gallery_list(f, app, area),
        AppMode::GalleryDetail => render_gallery_detail(f, app, area),
        AppMode::PageView => render_page_view(f, app, area),
    }
}

fn render_gallery_list(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app.galleries
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let style = if i == app.gallery_list_selected {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let content = format!("{} | {} pages | {}", g.id, g.page_count, g.title);
            ListItem::new(Line::from(content)).style(style)
        })
        .collect();

    let title = if app.galleries.is_empty() {
        "Gallery List (empty - press 's' to search)".to_string()
    } else {
        format!("Gallery List ({} galleries) - ↑/↓ Navigate, Enter Open, 's' Search, 'r' Refresh", app.galleries.len())
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, area, &mut app.gallery_list_state);
}

fn render_gallery_detail(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(6),
        Constraint::Min(5),
    ]).split(area);

    if let Some(g) = &app.selected_gallery {
        let info_text = format!(
            "ID: {}\nTitle: {}\nPages: {}\nUploader: {}\nPosted: {}\nURL: {}",
            g.id, g.title, g.page_count, g.uploader, g.posted, g.url
        );
        let info = Paragraph::new(info_text)
            .style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL).title("Gallery Info"))
            .wrap(Wrap { trim: true });
        f.render_widget(info, chunks[0]);
    }

    let items: Vec<ListItem> = app.gallery_pages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let style = if i == app.page_selected {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let content = format!("Page {}: {} | {}", i + 1, p.filename, p.image_url);
            ListItem::new(Line::from(content)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Pages - ↑/↓ Navigate, Enter View, 'b' Back"))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[1], &mut app.page_state);
}

fn render_page_view(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(8),
        Constraint::Min(5),
    ]).split(area);

    if let Some(page) = &app.current_page {
        let info_text = format!(
            "Page: {} / {}\nFilename: {}\nImage URL: {}\nSize: {} bytes\nGallery: {}\n\nPress 'b' or Esc to go back",
            app.page_selected + 1,
            app.gallery_pages.len(),
            page.filename,
            page.image_url,
            page.size.unwrap_or(0),
            app.selected_gallery.as_ref().map(|g| g.title.as_str()).unwrap_or(&"Unknown".to_string())
        );
        let info = Paragraph::new(info_text)
            .style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL).title("Page Info"))
            .wrap(Wrap { trim: true });
        f.render_widget(info, chunks[0]);

        // Image preview area
        let img_block = Block::default()
            .borders(Borders::ALL)
            .title("Image Preview (terminal image support required)")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(img_block, chunks[1]);

        let url_text = Paragraph::new(page.image_url.clone())
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().borders(Borders::NONE));
        let inner = chunks[1].inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(url_text, inner);
    }
}

fn render_status(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let status_style = if app.last_error.is_some() {
        Style::default().fg(Color::Red).bold()
    } else {
        Style::default().fg(Color::Green)
    };

    let status = Paragraph::new(app.status_message.as_str())
        .style(status_style)
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(status, area);
}

fn render_log_bar(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let visible_logs = 7;
    let start = app.logs.len().saturating_sub(visible_logs + app.log_scroll);
    let end = app.logs.len().saturating_sub(app.log_scroll);
    let logs_to_show = &app.logs[start..end.min(app.logs.len())];

    let lines: Vec<Line> = logs_to_show.iter().map(|log| {
        let status_color = if log.is_error { Color::Red } else { Color::Green };
        let time = Span::styled(log.timestamp.clone(), Style::default().fg(Color::DarkGray));
        let method = Span::styled(format!(" {:4} ", log.method), Style::default().fg(Color::Cyan).bold());
        let status = Span::styled(format!(" {} ", log.status), Style::default().fg(status_color).bold());
        let duration = Span::styled(format!(" {}ms ", log.duration_ms), Style::default().fg(Color::Yellow));
        let url = Span::styled(log.url.clone(), Style::default().fg(Color::White));
        Line::from(vec![time, method, status, duration, url])
    }).collect();

    let log_text = Text::from(lines);
    let log_block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Request Log (h/l to scroll, {} entries)", app.logs.len()));

    let paragraph = Paragraph::new(log_text)
        .block(log_block)
        .wrap(Wrap { trim: false });

    let mut scrollbar_state = ScrollbarState::default()
        .content_length(app.logs.len())
        .position(app.log_scroll);

    f.render_widget(paragraph, area);

    let scrollbar_area = Rect {
        x: area.right().saturating_sub(1),
        y: area.top() + 1,
        width: 1,
        height: area.height.saturating_sub(2),
    };
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(Color::DarkGray))
            .thumb_symbol("█")
            .track_symbol(Some("│")),
        scrollbar_area,
        &mut scrollbar_state,
    );
}
