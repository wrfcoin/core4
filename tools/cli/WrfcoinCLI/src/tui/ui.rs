use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use super::app::{App, ConnectionStatus, Tab};
use super::tabs;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    draw_tab_bar(f, app, chunks[0]);
    draw_content(f, app, chunks[1]);
    draw_status_bar(f, app, chunks[2]);
}

fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| {
            let style = if *t == app.current_tab {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(Span::styled(format!(" {} ", t.title()), style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" WRFCoin TUI "),
        )
        .select(app.current_tab.index())
        .highlight_style(Style::default().fg(Color::Yellow));

    f.render_widget(tabs, area);
}

fn draw_content(f: &mut Frame, app: &App, area: Rect) {
    match app.current_tab {
        Tab::Dashboard => tabs::dashboard::render(f, area, app),
        Tab::Weather => tabs::weather::render(f, area, app),
        Tab::Network => tabs::network::render(f, area, app),
        Tab::Wallet => tabs::wallet::render(f, area, app),
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
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
            format!("Refresh in {}s", next_refresh),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  |  "),
        Span::styled(
            "q:quit  Tab:switch  1-4:tab  r:refresh",
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    f.render_widget(status, area);
}
