use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Sparkline};
use serde_json::Value;

use crate::tui::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // Node status
            Constraint::Length(7), // Mining stats
            Constraint::Min(5),    // Hashrate sparkline
        ])
        .split(area);

    render_node_status(f, chunks[0], app);
    render_mining_stats(f, chunks[1], app);
    render_hashrate_sparkline(f, chunks[2], app);
}

fn render_node_status(f: &mut Frame, area: Rect, app: &App) {
    let text = if let Some(ref status) = app.blockchain_status {
        let height = status
            .get("block_height")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let txs = status
            .get("transactions_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let nodes = status
            .get("nodes_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let network = status
            .get("network")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let version = status
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        vec![
            Line::from(vec![
                Span::styled("Network: ", Style::default().fg(Color::DarkGray)),
                Span::styled(network, Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("Consensus: ", Style::default().fg(Color::DarkGray)),
                Span::styled(version, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Block Height: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", height),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Transactions: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}", txs), Style::default().fg(Color::White)),
                Span::raw("  "),
                Span::styled("Nodes: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}", nodes), Style::default().fg(Color::White)),
            ]),
        ]
    } else {
        vec![Line::from(Span::styled(
            "Node status unavailable",
            Style::default().fg(Color::Red),
        ))]
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Node Status ");
    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

fn render_mining_stats(f: &mut Frame, area: Rect, app: &App) {
    let text = if let Some(ref stats) = app.mining_stats {
        let hashrate = stats.get("hashrate").and_then(Value::as_u64).unwrap_or(0);
        let difficulty = stats.get("difficulty").and_then(Value::as_u64).unwrap_or(0);
        let last_block = stats
            .get("last_block_height")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let avg_block_time = stats
            .get("avg_block_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);

        vec![
            Line::from(vec![
                Span::styled("Hashrate: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format_hashrate(hashrate),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Difficulty: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}", difficulty), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Last Block: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("#{}", last_block),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("Avg Block Time: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if avg_block_time > 0.0 {
                        format!("{avg_block_time:.0}s")
                    } else {
                        "n/a".to_string()
                    },
                    Style::default().fg(Color::White),
                ),
            ]),
        ]
    } else {
        vec![Line::from(Span::styled(
            "Mining stats unavailable",
            Style::default().fg(Color::Red),
        ))]
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Mining Stats ");
    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

fn render_hashrate_sparkline(f: &mut Frame, area: Rect, app: &App) {
    let data: Vec<u64> = app.hashrate_history.iter().copied().collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Hashrate History ");

    if data.is_empty() {
        let paragraph = Paragraph::new("No hashrate data yet").block(block);
        f.render_widget(paragraph, area);
    } else {
        let sparkline = Sparkline::default()
            .block(block)
            .data(&data)
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(sparkline, area);
    }
}

fn format_hashrate(h: u64) -> String {
    if h >= 1_000_000_000 {
        format!("{:.2} GH/s", h as f64 / 1_000_000_000.0)
    } else if h >= 1_000_000 {
        format!("{:.2} MH/s", h as f64 / 1_000_000.0)
    } else if h >= 1_000 {
        format!("{:.2} KH/s", h as f64 / 1_000.0)
    } else {
        format!("{} H/s", h)
    }
}
