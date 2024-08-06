use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use ratatui::{
    layout::{
        Constraint,
        Direction,
        Layout,
    },
    style::{
        Color,
        Modifier,
        Style,
    },
    text::{
        Line,
        Span,
        Text,
    },
    widgets::{
        Block,
        Borders,
        Cell,
        Paragraph,
        Row,
        Table,
    },
    Frame,
};

/// Draws the UI components.
pub fn draw_ui(
    f: &mut Frame, configs: &[Config], config_states: &[ConfigState], selected_row: usize,
    show_details: bool,
) {
    let size = f.size();
    let chunks = if show_details {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(60),
                    Constraint::Percentage(30),
                    Constraint::Percentage(10),
                ]
                .as_ref(),
            )
            .split(size)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(85), Constraint::Percentage(15)].as_ref())
            .split(size)
    };

    let rows: Vec<Row> = configs
        .iter()
        .enumerate()
        .map(|(row_index, config)| {
            let state = config_states
                .iter()
                .find(|s| s.config_id == config.id.unwrap_or_default())
                .map(|s| s.is_running)
                .unwrap_or(false);

            let style = if row_index == selected_row {
                Style::default().fg(Color::White).bg(Color::Blue)
            } else {
                Style::default().fg(if state { Color::Green } else { Color::Red })
            };

            let remote_target = match config.workload_type.as_str() {
                "proxy" => config.remote_address.clone().unwrap_or_default(),
                "service" => config.service.clone().unwrap_or_default(),
                "pod" => config.target.clone().unwrap_or_default(),
                _ => String::new(),
            };

            Row::new(vec![
                Cell::from(config.alias.clone().unwrap_or_default()),
                Cell::from(config.workload_type.clone()),
                Cell::from(remote_target),
                Cell::from(config.local_address.clone().unwrap_or_default()),
                Cell::from(config.local_port.to_string()),
                Cell::from(state.to_string()),
                Cell::from(config.context.clone()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        &[
            Constraint::Length(30),
            Constraint::Length(30),
            Constraint::Length(50),
            Constraint::Length(30),
            Constraint::Length(15),
            Constraint::Length(30),
            Constraint::Length(30),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from(Span::styled(
                "Alias",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Workload",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Remote Target",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Local Address",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Local Port",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Is Running",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Context",
                Style::default().add_modifier(Modifier::BOLD),
            )),
        ])
        .style(Style::default().bg(Color::DarkGray)),
    )
    .block(Block::default().borders(Borders::ALL).title("kftray tui"))
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_symbol(">> ");

    f.render_widget(table, chunks[0]);

    if show_details {
        render_details(f, &configs[selected_row], config_states, chunks[1]);
    }

    render_help(f, chunks[chunks.len() - 1]);
}

/// Renders the details section.
fn render_details(
    f: &mut Frame, config: &Config, config_states: &[ConfigState], area: ratatui::layout::Rect,
) {
    let state = config_states
        .iter()
        .find(|s| s.config_id == config.id.unwrap_or_default())
        .map(|s| s.is_running)
        .unwrap_or(false);

    let details = vec![
        Line::from(vec![
            Span::styled("Context: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&config.context),
        ]),
        Line::from(vec![
            Span::styled("Alias: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(config.alias.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Service: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(config.service.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Namespace: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&config.namespace),
        ]),
        Line::from(vec![
            Span::styled(
                "Local Address: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.local_address.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled(
                "Local Port: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.local_port.to_string()),
        ]),
        Line::from(vec![
            Span::styled(
                "Remote Address: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.remote_address.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled(
                "Remote Port: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.remote_port.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Context: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&config.context),
        ]),
        Line::from(vec![
            Span::styled(
                "Workload Type: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(&config.workload_type),
        ]),
        Line::from(vec![
            Span::styled("Protocol: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&config.protocol),
        ]),
        Line::from(vec![
            Span::styled(
                "Domain Enabled: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.domain_enabled.unwrap_or(false).to_string()),
        ]),
        Line::from(vec![
            Span::styled(
                "Kubeconfig: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.kubeconfig.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Target: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(config.target.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled(
                "Is Running: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(state.to_string()),
        ]),
    ];

    let paragraph = Paragraph::new(Text::from(details))
        .block(Block::default().borders(Borders::ALL).title("Details"))
        .style(Style::default().fg(Color::White));

    f.render_widget(paragraph, area);
}

/// Renders the help section.
fn render_help(f: &mut Frame, area: ratatui::layout::Rect) {
    let help_message = vec![
        Line::from(Span::styled("q: Quit", Style::default().fg(Color::Yellow))),
        Line::from(Span::styled(
            "↑/↓: Navigate",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "Enter: Toggle Details",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "f: Toggle Port Forward",
            Style::default().fg(Color::Yellow),
        )),
    ];

    let help_paragraph = Paragraph::new(Text::from(help_message))
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .style(Style::default().fg(Color::White));

    f.render_widget(help_paragraph, area);
}
