use std::collections::HashSet;

use clap::Parser;
use crossterm::event::KeyCode;
use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use kftray_commons::utils::config::{
    get_config_with_mode,
    insert_config_with_mode,
};
use kftray_commons::utils::config_view::{
    ConfigView,
    Field,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::widgets::TableState;

use crate::cli::Cli;
use crate::cli::runner::PortForwardRunner;
use crate::tests::test_logger_state;
use crate::tui::input::{
    App,
    AppState,
    ViewItem,
    handle_tag_editor_input,
    handle_view_settings_input,
    open_tag_editor,
    view_items,
};
use crate::tui::ui::{
    draw_configs_table,
    render_view_settings_popup,
};

fn config(id: i64, alias: &str, tags: &[(&str, &str)]) -> Config {
    Config {
        id: Some(id),
        alias: Some(alias.to_string()),
        context: Some("ctx".to_string()),
        namespace: "default".to_string(),
        protocol: "tcp".to_string(),
        tags: tags
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..Config::default()
    }
}

fn new_config(alias: &str, tags: &[(&str, &str)]) -> Config {
    Config {
        id: None,
        ..config(0, alias, tags)
    }
}

fn sample() -> Vec<Config> {
    vec![
        config(1, "b-api", &[("team", "payments")]),
        config(2, "a-db", &[("team", "core")]),
        config(3, "c-web", &[("team", "payments"), ("pinned", "")]),
        config(4, "d-cache", &[]),
    ]
}

fn stopped_states(configs: &[Config]) -> Vec<ConfigState> {
    configs
        .iter()
        .map(|c| ConfigState {
            config_id: c.id.unwrap(),
            is_running: false,
            ..Default::default()
        })
        .collect()
}

fn ids(configs: &[Config]) -> Vec<i64> {
    configs.iter().filter_map(|c| c.id).collect()
}

#[test]
fn view_filters_and_orders_the_tables() {
    let mut app = App::new(test_logger_state());
    let configs = sample();
    app.config_view = ConfigView {
        group_by: Some(Field::Tag("team".into())),
        filters: vec!["tag:team".parse().unwrap()],
    };
    app.update_configs(&configs, &stopped_states(&configs));

    assert_eq!(ids(&app.stopped_configs), vec![2, 1, 3]);
    assert_eq!(app.group_labels.get(&2).map(String::as_str), Some("core"));
    assert_eq!(
        app.group_labels.get(&3).map(String::as_str),
        Some("payments")
    );
    assert_eq!(app.all_configs.len(), 4);

    app.config_view.filters = vec!["tag:team=payments".parse().unwrap()];
    app.update_configs(&configs, &stopped_states(&configs));
    assert_eq!(ids(&app.stopped_configs), vec![1, 3]);
}

#[test]
fn shrinking_the_list_clamps_selection() {
    let mut app = App::new(test_logger_state());
    let configs = sample();
    app.update_configs(&configs, &stopped_states(&configs));
    app.selected_rows_stopped = [0, 3].into();
    app.selected_row_stopped = 3;
    app.table_state_stopped.select(Some(3));

    app.config_view.filters = vec!["tag:pinned".parse().unwrap()];
    app.update_configs(&configs, &stopped_states(&configs));

    assert_eq!(ids(&app.stopped_configs), vec![3]);
    assert_eq!(app.selected_rows_stopped, [0].into());
    assert_eq!(app.selected_row_stopped, 0);
    assert_eq!(app.table_state_stopped.selected(), Some(0));
}

fn render_table(app: &App, grouped: bool) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(100, 10)).unwrap();
    let mut state = TableState::default();
    terminal
        .draw(|frame| {
            draw_configs_table(
                frame,
                frame.area(),
                &app.stopped_configs,
                &[],
                &mut state,
                "Stopped",
                false,
                &HashSet::new(),
                &Default::default(),
                &Default::default(),
                grouped.then_some(&app.group_labels),
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn group_column_labels_only_first_row_of_each_group() {
    let mut app = App::new(test_logger_state());
    let configs = sample();
    app.config_view.group_by = Some(Field::Tag("team".into()));
    app.update_configs(&configs, &stopped_states(&configs));

    let lines = render_table(&app, true);
    let first_cells: Vec<String> = lines[2..6]
        .iter()
        .map(|line| {
            line.chars()
                .skip(1)
                .take(18)
                .collect::<String>()
                .trim()
                .to_string()
        })
        .collect();
    assert!(lines[1].contains("Group"));
    assert_eq!(first_cells, vec!["core", "payments", "", "Ungrouped"]);

    let lines = render_table(&app, false);
    assert!(!lines[1].contains("Group"));
}

#[test]
fn view_items_toggle_filters() {
    let configs = sample();
    let items = view_items(&configs);
    assert!(items.contains(&ViewItem::GroupBy(None)));
    assert!(items.contains(&ViewItem::GroupBy(Some(Field::Tag("pinned".into())))));
    assert!(items.contains(&ViewItem::HasTag("pinned".into())));
    assert!(items.contains(&ViewItem::Value(Field::Tag("team".into()), "core".into())));

    let mut view = ConfigView::default();
    let team = Field::Tag("team".into());
    ViewItem::Value(team.clone(), "core".into()).apply(&mut view);
    ViewItem::Value(team.clone(), "payments".into()).apply(&mut view);
    assert_eq!(view.filters[0].to_string(), "tag:team=core,payments");
    assert!(
        ViewItem::Value(team.clone(), "core".into())
            .label(&view)
            .starts_with("[x]")
    );

    ViewItem::HasTag("team".into()).apply(&mut view);
    assert_eq!(view.filters.len(), 1);
    assert!(view.is_present_filter(&team));
    ViewItem::HasTag("team".into()).apply(&mut view);
    assert!(view.filters.is_empty());

    ViewItem::GroupBy(None).apply(&mut view);
    assert_eq!(view.group_by, None);
}

#[tokio::test]
async fn tag_editor_saves_parsed_tags() {
    let mode = DatabaseMode::Memory;
    let original = new_config("tag-editor", &[("old", "")]);
    let id = insert_config_with_mode(original.clone(), mode)
        .await
        .unwrap();
    let mut app = App::new(test_logger_state());
    app.stopped_configs = vec![Config {
        id: Some(id),
        ..original
    }];
    app.table_state_stopped.select(Some(0));
    app.selected_rows_stopped = HashSet::from([0]);

    open_tag_editor(&mut app);
    assert_eq!(app.state, AppState::ShowTagEditor);
    assert_eq!(app.input_buffer, "old");

    app.input_buffer = "Env=dev, pinned".to_string();
    handle_tag_editor_input(&mut app, KeyCode::Enter, mode)
        .await
        .unwrap();
    assert_eq!(app.state, AppState::Normal);
    assert!(app.selected_rows_stopped.is_empty());

    let saved = get_config_with_mode(id, mode).await.unwrap();
    assert_eq!(
        saved.tags,
        [
            ("env".to_string(), "dev".to_string()),
            ("pinned".to_string(), String::new())
        ]
        .into()
    );
}

#[tokio::test]
async fn changing_the_view_drops_marked_rows() {
    let mut app = App::new(test_logger_state());
    app.all_configs = vec![config(1, "a", &[("team", "core")])];
    app.selected_rows_stopped = HashSet::from([0, 2]);
    app.selected_rows_running = HashSet::from([1]);
    app.state = AppState::ShowViewSettings;

    handle_view_settings_input(&mut app, KeyCode::Char('c'), DatabaseMode::Memory)
        .await
        .unwrap();

    assert!(app.selected_rows_stopped.is_empty());
    assert!(app.selected_rows_running.is_empty());
}

#[tokio::test]
async fn moving_in_the_view_popup_keeps_marked_rows() {
    let mut app = App::new(test_logger_state());
    app.selected_rows_stopped = HashSet::from([0]);
    app.state = AppState::ShowViewSettings;

    handle_view_settings_input(&mut app, KeyCode::Down, DatabaseMode::Memory)
        .await
        .unwrap();

    assert_eq!(app.selected_rows_stopped, HashSet::from([0]));
}

#[tokio::test]
async fn tag_editor_reports_invalid_tags() {
    let mut app = App::new(test_logger_state());
    app.stopped_configs = vec![config(1, "a", &[])];
    open_tag_editor(&mut app);
    app.input_buffer = "bad key=x".to_string();

    handle_tag_editor_input(&mut app, KeyCode::Enter, DatabaseMode::Memory)
        .await
        .unwrap();

    assert_eq!(app.state, AppState::ShowErrorPopup);
    assert!(app.error_message.unwrap().contains("tag key"));
}

#[test]
fn cli_parses_filter_and_group_by() {
    let cli = Cli::try_parse_from([
        "kftui",
        "--filter",
        "tag:env=dev",
        "--filter",
        "namespace=a,b",
        "--group-by",
        "tag:team",
    ])
    .unwrap();
    assert_eq!(
        cli.filters
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["tag:env=dev", "namespace=a,b"]
    );

    let view = cli.apply_view_overrides(ConfigView::default());
    assert_eq!(view.group_by, Some(Field::Tag("team".into())));
    assert_eq!(view.filters, cli.filters);

    let cli = Cli::try_parse_from(["kftui", "--group-by", "none"]).unwrap();
    let view = cli.apply_view_overrides(ConfigView {
        group_by: Some(Field::Context),
        filters: vec!["tag:x".parse().unwrap()],
    });
    assert_eq!(view.group_by, None);
    assert_eq!(view.filters.len(), 1);

    assert!(Cli::try_parse_from(["kftui", "--filter", "bogus=1"]).is_err());
    assert!(Cli::try_parse_from(["kftui", "--group-by", "alias"]).is_err());
}

#[test]
fn auto_start_only_selects_configs_matching_filters() {
    let configs = sample();

    let cli = Cli::try_parse_from(["kftui", "-a", "--filter", "tag:team=payments"]).unwrap();
    assert_eq!(
        PortForwardRunner::select_config_ids(&cli, &configs, Vec::new()),
        vec![1, 3]
    );

    let cli = Cli::try_parse_from(["kftui", "-a", "--json", "[]", "--filter", "tag:team"]).unwrap();
    assert_eq!(
        PortForwardRunner::select_config_ids(&cli, &configs, vec![2, 3, 4]),
        vec![2, 3]
    );
}

#[test]
fn view_popup_lists_group_by_and_filter_rows() {
    let mut app = App::new(test_logger_state());
    let configs = sample();
    app.update_configs(&configs, &stopped_states(&configs));
    app.config_view.filters = vec!["tag:team=core".parse().unwrap()];

    let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    terminal
        .draw(|frame| render_view_settings_popup(frame, &app, frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let screen: String = buffer.content.iter().map(|cell| cell.symbol()).collect();

    assert!(screen.contains("(•) group by context"));
    assert!(screen.contains("[x] tag:team = core"));
    assert!(screen.contains("[ ] has tag pinned"));
}
