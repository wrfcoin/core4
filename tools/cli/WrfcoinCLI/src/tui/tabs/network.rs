use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use serde_json::Value;

use crate::tui::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50), // Peers
            Constraint::Percentage(50), // Bridges
        ])
        .split(area);

    render_peers(f, chunks[0], app);
    render_bridges(f, chunks[1], app);
}

fn render_peers(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" Peers ");

    if let Some(ref peers) = app.peers {
        if let Some(peer_list) = peers.as_array() {
            if peer_list.is_empty() {
                let paragraph = Paragraph::new("No peers connected").block(block);
                f.render_widget(paragraph, area);
                return;
            }

            let header = Row::new(vec!["Address", "Status", "Version"]).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

            let rows: Vec<Row> = peer_list
                .iter()
                .take(15)
                .map(|peer| {
                    let addr = peer
                        .get("address")
                        .or(peer.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let status = peer
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("active");
                    let version = peer.get("version").and_then(Value::as_str).unwrap_or("-");

                    let status_style = match status {
                        "active" | "connected" => Style::default().fg(Color::Green),
                        _ => Style::default().fg(Color::Yellow),
                    };

                    Row::new(vec![
                        Cell::from(addr.to_string()),
                        Cell::from(Span::styled(status, status_style)),
                        Cell::from(version.to_string()),
                    ])
                })
                .collect();

            let widths = [
                Constraint::Percentage(50),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ];
            let table = Table::new(rows, widths).header(header).block(block);
            f.render_widget(table, area);
        } else if let Some(count) = peers.get("connected_peers").and_then(Value::as_u64) {
            let local_peer = peers
                .get("local_peer_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let paragraph = Paragraph::new(vec![
                Line::from(format!("Connected peers: {count}")),
                Line::from(format!("Local peer ID: {local_peer}")),
            ])
            .block(block);
            f.render_widget(paragraph, area);
        } else {
            let paragraph = Paragraph::new("Peer data format unexpected").block(block);
            f.render_widget(paragraph, area);
        }
    } else {
        let paragraph = Paragraph::new(Span::styled(
            "Peer data unavailable",
            Style::default().fg(Color::Red),
        ))
        .block(block);
        f.render_widget(paragraph, area);
    }
}

fn render_bridges(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Bridge Status ");

    if let Some(ref bridges) = app.bridge_status {
        if let Some(bridge_list) = bridges.as_array() {
            let mut lines = Vec::new();
            for bridge in bridge_list.iter().take(10) {
                let name = bridge
                    .get("name")
                    .or(bridge.get("chain"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let status = bridge
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let status_color = match status {
                    "active" | "connected" | "operational" => Color::Green,
                    "degraded" | "syncing" => Color::Yellow,
                    _ => Color::Red,
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("{}: ", name), Style::default().fg(Color::White)),
                    Span::styled(status, Style::default().fg(status_color)),
                ]));
            }

            if lines.is_empty() {
                lines.push(Line::from("No bridges configured"));
            }

            let paragraph = Paragraph::new(lines).block(block);
            f.render_widget(paragraph, area);
        } else {
            // bridges might be a single object
            let status = bridges
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let paragraph = Paragraph::new(format!("Bridge status: {}", status)).block(block);
            f.render_widget(paragraph, area);
        }
    } else {
        let paragraph = Paragraph::new(Span::styled(
            "Bridge data unavailable",
            Style::default().fg(Color::Red),
        ))
        .block(block);
        f.render_widget(paragraph, area);
    }
}
