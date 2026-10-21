use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::io::{self, IsTerminal};

use super::app::{App, ConnectionStatus, Tab};
use super::events::{self, AppEvent};
use super::tabs;

#[derive(Clone, Copy, PartialEq)]
pub enum LiveLayout {
    Quad,
    Vertical,
    Horizontal,
    Focus,
}

impl LiveLayout {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "vertical" | "v" => LiveLayout::Vertical,
            "horizontal" | "h" => LiveLayout::Horizontal,
            "focus" | "f" => LiveLayout::Focus,
            _ => LiveLayout::Quad,
        }
    }

    fn next(self) -> Self {
        match self {
            LiveLayout::Quad => LiveLayout::Vertical,
            LiveLayout::Vertical => LiveLayout::Horizontal,
            LiveLayout::Horizontal => LiveLayout::Focus,
            LiveLayout::Focus => LiveLayout::Quad,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LiveLayout::Quad => "Quad (2x2)",
            LiveLayout::Vertical => "Vertical",
            LiveLayout::Horizontal => "Horizontal",
            LiveLayout::Focus => "Focus",
        }
    }
}

pub struct LiveState {
    pub layout: LiveLayout,
    pub focused_pane: usize,
    pub show_help: bool,
}

impl LiveState {
    fn new(layout: LiveLayout) -> Self {
        Self {
            layout,
            focused_pane: 0,
            show_help: false,
        }
    }
}

const PANE_TABS: [Tab; 4] = [Tab::Dashboard, Tab::Weather, Tab::Network, Tab::Wallet];

pub async fn run_live(
    interval: u64,
    station_id: Option<String>,
    address: Option<String>,
    layout: &str,
) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        anyhow::bail!(
            "Live dashboard requires an interactive terminal. Use --json for non-interactive output."
        );
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(interval, station_id, address);
    let mut state = LiveState::new(LiveLayout::from_str(layout));

    // Check terminal size — fall back to single-tab TUI if too small
    let size = terminal.size()?;
    if size.width < 100 || size.height < 30 {
        // Restore terminal and fall back
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        return super::run_tui(app.interval, app.station_id.clone(), app.address.clone()).await;
    }

    app.fetch_data().await;

    let result = run_live_loop(&mut terminal, &mut app, &mut state).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_live_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    state: &mut LiveState,
) -> Result<()> {
    let render_tick = std::time::Duration::from_millis(250);

    loop {
        terminal.draw(|f| draw_live(f, app, state))?;

        match events::poll_event(render_tick)? {
            AppEvent::Key(key) => {
                if on_live_key(key, app, state) {
                    return Ok(());
                }
            },
            AppEvent::Tick => {},
        }

        if app.should_refresh() {
            app.fetch_data().await;
        }
    }
}

/// Handle key events for live mode. Returns true to quit.
fn on_live_key(key: KeyEvent, app: &mut App, state: &mut LiveState) -> bool {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => true,
        KeyCode::Char('l') => {
            state.layout = state.layout.next();
            false
        },
        KeyCode::Tab => {
            state.focused_pane = (state.focused_pane + 1) % 4;
            false
        },
        KeyCode::BackTab => {
            state.focused_pane = (state.focused_pane + 3) % 4;
            false
        },
        KeyCode::Char('?') => {
            state.show_help = !state.show_help;
            false
        },
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if app.interval > 1 {
                app.interval -= 1;
            }
            false
        },
        KeyCode::Char('-') => {
            if app.interval < 300 {
                app.interval += 1;
            }
            false
        },
        KeyCode::Char('r') => {
            app.last_refresh =
                std::time::Instant::now() - std::time::Duration::from_secs(app.interval + 1);
            false
        },
        KeyCode::Char('1') => {
            state.focused_pane = 0;
            false
        },
        KeyCode::Char('2') => {
            state.focused_pane = 1;
            false
        },
        KeyCode::Char('3') => {
            state.focused_pane = 2;
            false
        },
        KeyCode::Char('4') => {
            state.focused_pane = 3;
            false
        },
        _ => false,
    }
}

fn draw_live(f: &mut Frame, app: &App, state: &LiveState) {
    let full = f.area();

    // Reserve 1 row for status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(full);

    let content_area = chunks[0];
    let status_area = chunks[1];

    // Split content into panes based on layout
    let pane_areas = split_for_layout(state.layout, content_area);

    // Render each pane
    for (i, &area) in pane_areas.iter().enumerate() {
        let tab = PANE_TABS[i];
        let is_focused = i == state.focused_pane;
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Create a block with the pane title
        let title = format!(" {} ", tab.title());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        let inner = block.inner(area);
        f.render_widget(block, area);

        // Render tab content into inner area
        match tab {
            Tab::Dashboard => tabs::dashboard::render(f, inner, app),
            Tab::Weather => tabs::weather::render(f, inner, app),
            Tab::Network => tabs::network::render(f, inner, app),
            Tab::Wallet => tabs::wallet::render(f, inner, app),
        }
    }

    // Status bar
    draw_live_status(f, app, state, status_area);

    // Help overlay
    if state.show_help {
        draw_help_overlay(f, full);
    }
}

fn split_for_layout(layout: LiveLayout, area: Rect) -> Vec<Rect> {
    match layout {
        LiveLayout::Quad => {
            // 2x2 grid
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            let top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(rows[0]);
            let bottom = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(rows[1]);
            vec![top[0], top[1], bottom[0], bottom[1]]
        },
        LiveLayout::Vertical => {
            // 4 vertical strips
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ])
                .split(area);
            cols.to_vec()
        },
        LiveLayout::Horizontal => {
            // 4 horizontal strips
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ])
                .split(area);
            rows.to_vec()
        },
        LiveLayout::Focus => {
            // 1 large pane on left, 3 small on right sidebar
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(area);
            let sidebar = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(33),
                    Constraint::Percentage(34),
                    Constraint::Percentage(33),
                ])
                .split(cols[1]);
            vec![cols[0], sidebar[0], sidebar[1], sidebar[2]]
        },
    }
}

fn draw_live_status(f: &mut Frame, app: &App, state: &LiveState, area: Rect) {
    let (status_text, status_color) = match app.connection_status {
        ConnectionStatus::Connected => ("● Connected", Color::Green),
        ConnectionStatus::Degraded => ("◐ Degraded", Color::Yellow),
        ConnectionStatus::Disconnected => ("○ Disconnected", Color::Red),
    };

    let elapsed = app.last_refresh.elapsed().as_secs();
    let next_refresh = if elapsed >= app.interval {
        0
    } else {
        app.interval - elapsed
    };

    let status = Paragraph::new(Line::from(vec![
        Span::styled(status_text, Style::default().fg(status_color)),
        Span::raw("  |  "),
        Span::styled(
            format!("Layout: {}", state.layout.label()),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("Refresh: {}s (in {}s)", app.interval, next_refresh),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  |  "),
        Span::styled(
            "q:quit  l:layout  Tab:focus  +/-:interval  ?:help",
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    f.render_widget(status, area);
}

fn draw_help_overlay(f: &mut Frame, full: Rect) {
    let help_width = 50u16;
    let help_height = 14u16;
    let x = full.width.saturating_sub(help_width) / 2;
    let y = full.height.saturating_sub(help_height) / 2;
    let area = Rect::new(
        x,
        y,
        help_width.min(full.width),
        help_height.min(full.height),
    );

    f.render_widget(Clear, area);

    let help_text = vec![
        Line::from(Span::styled(
            "Live Dashboard Help",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  q / Esc     Quit"),
        Line::from("  l           Cycle layout (quad/vert/horiz/focus)"),
        Line::from("  Tab         Cycle focused pane"),
        Line::from("  Shift+Tab   Cycle focused pane (reverse)"),
        Line::from("  1-4         Focus pane directly"),
        Line::from("  + / =       Decrease refresh interval"),
        Line::from("  -           Increase refresh interval"),
        Line::from("  r           Force refresh now"),
        Line::from("  ?           Toggle this help"),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Help ");
    let paragraph = Paragraph::new(help_text).block(block);
    f.render_widget(paragraph, area);
}
