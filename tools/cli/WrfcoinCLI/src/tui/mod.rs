pub mod app;
pub mod events;
pub mod live;
pub mod tabs;
pub mod ui;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use std::io::{self, IsTerminal};

use app::App;
use events::AppEvent;

pub async fn run_tui(
    interval: u64,
    station_id: Option<String>,
    address: Option<String>,
) -> Result<()> {
    // TTY guard
    if !std::io::stdout().is_terminal() {
        anyhow::bail!(
            "TUI requires an interactive terminal. Use --json for non-interactive output."
        );
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(interval, station_id, address);
    // Initial data fetch
    app.fetch_data().await;

    let result = run_event_loop(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let render_tick = std::time::Duration::from_millis(250);

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        match events::poll_event(render_tick)? {
            AppEvent::Key(key) => {
                if app.on_key(key) {
                    return Ok(());
                }
            },
            AppEvent::Tick => {},
        }

        // Check if it's time for a data refresh
        if app.should_refresh() {
            app.fetch_data().await;
        }
    }
}
