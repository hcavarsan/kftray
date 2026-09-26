use std::io;

use crossterm::event::KeyCode;
use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::update_config_with_mode;
use kftray_commons::utils::config_view::{
    ConfigView,
    Field,
    facets,
    format_tags,
    parse_tags,
    set_config_view_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;

use crate::tui::input::{
    App,
    AppState,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewItem {
    GroupBy(Option<Field>),
    HasTag(String),
    Value(Field, String),
    ClearFilters,
}

impl ViewItem {
    pub fn label(&self, view: &ConfigView) -> String {
        match self {
            ViewItem::GroupBy(field) => {
                let mark = if view.group_by == *field {
                    "(•)"
                } else {
                    "( )"
                };
                let name = field.as_ref().map_or("none".to_string(), Field::to_string);
                format!("{mark} group by {name}")
            }
            ViewItem::HasTag(key) => {
                let active = view.is_present_filter(&Field::Tag(key.clone()));
                format!("{} has tag {key}", checkbox(active))
            }
            ViewItem::Value(field, value) => {
                let active = view.is_value_filter(field, value);
                format!("{} {field} = {value}", checkbox(active))
            }
            ViewItem::ClearFilters => format!("clear all filters ({})", view.filters.len()),
        }
    }

    pub fn apply(&self, view: &mut ConfigView) {
        match self {
            ViewItem::GroupBy(field) => view.group_by = field.clone(),
            ViewItem::HasTag(key) => view.toggle_present(&Field::Tag(key.clone())),
            ViewItem::Value(field, value) => view.toggle_value(field, value),
            ViewItem::ClearFilters => view.filters.clear(),
        }
    }
}

fn checkbox(active: bool) -> &'static str {
    if active { "[x]" } else { "[ ]" }
}

pub fn view_items(configs: &[Config]) -> Vec<ViewItem> {
    let facets = facets(configs);
    let mut items = vec![ViewItem::GroupBy(None)];
    items.extend(
        facets
            .iter()
            .map(|facet| ViewItem::GroupBy(Some(facet.field.clone()))),
    );
    for facet in &facets {
        if let Field::Tag(key) = &facet.field {
            items.push(ViewItem::HasTag(key.clone()));
        }
        items.extend(
            facet
                .values
                .iter()
                .map(|v| ViewItem::Value(facet.field.clone(), v.value.clone())),
        );
    }
    items.push(ViewItem::ClearFilters);
    items
}

pub fn open_view_settings(app: &mut App) {
    app.view_selected = 0;
    app.state = AppState::ShowViewSettings;
}

pub async fn handle_view_settings_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    let items = view_items(&app.all_configs);
    let last = items.len().saturating_sub(1);
    app.view_selected = app.view_selected.min(last);

    let changed = match key {
        KeyCode::Esc | KeyCode::Char('v') => {
            app.state = AppState::Normal;
            false
        }
        KeyCode::Up => {
            app.view_selected = app.view_selected.saturating_sub(1);
            false
        }
        KeyCode::Down => {
            app.view_selected = (app.view_selected + 1).min(last);
            false
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            items[app.view_selected].apply(&mut app.config_view);
            true
        }
        KeyCode::Char('c') => {
            ViewItem::ClearFilters.apply(&mut app.config_view);
            true
        }
        _ => false,
    };

    if !changed {
        return Ok(());
    }
    clear_marked_rows(app);
    if let Err(e) = set_config_view_with_mode(&app.config_view, mode).await {
        app.error_message = Some(format!("Failed to save view: {e}"));
        app.state = AppState::ShowErrorPopup;
    }
    Ok(())
}

/// Marked rows are stored as row indices, so anything that reorders or
/// refilters the tables has to drop them. Otherwise a bulk delete or start
/// would act on whatever configs moved into those rows.
fn clear_marked_rows(app: &mut App) {
    app.selected_rows_stopped.clear();
    app.selected_rows_running.clear();
}

pub fn open_tag_editor(app: &mut App) {
    let Some(config) = app.selected_config().cloned() else {
        return;
    };
    app.input_buffer = format_tags(&config.tags);
    app.tag_editor_config = Some(config);
    app.state = AppState::ShowTagEditor;
}

pub async fn handle_tag_editor_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    match key {
        KeyCode::Enter => save_tags(app, mode).await,
        KeyCode::Char(c) => app.input_buffer.push(c),
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Esc => {
            app.tag_editor_config = None;
            app.input_buffer.clear();
            app.state = AppState::Normal;
        }
        _ => {}
    }
    Ok(())
}

pub async fn save_tags(app: &mut App, mode: DatabaseMode) {
    let input = std::mem::take(&mut app.input_buffer);
    let Some(mut config) = app.tag_editor_config.take() else {
        app.state = AppState::Normal;
        return;
    };

    let result = match parse_tags(&input) {
        Ok(tags) => {
            config.tags = tags;
            update_config_with_mode(config, mode).await
        }
        Err(e) => Err(e),
    };

    match result {
        Ok(()) => {
            clear_marked_rows(app);
            app.state = AppState::Normal;
        }
        Err(e) => {
            app.error_message = Some(format!("Failed to save tags: {e}"));
            app.state = AppState::ShowErrorPopup;
        }
    }
}
