use std::collections::{
    BTreeMap,
    BTreeSet,
};
use std::fmt;
use std::str::FromStr;

use log::warn;
use serde::{
    Deserialize,
    Serialize,
};

use crate::models::config_model::Config;
use crate::utils::db_mode::DatabaseMode;
use crate::utils::settings::{
    get_setting_with_mode,
    set_setting_with_mode,
};

const CONFIG_VIEW_KEY: &str = "config_view";
const TAG_PREFIX: &str = "tag:";
const MAX_TAG_KEY_LEN: usize = 63;
const MAX_TAG_VALUE_LEN: usize = 128;
pub const UNGROUPED_LABEL: &str = "Ungrouped";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Field {
    Context,
    Namespace,
    Kubeconfig,
    WorkloadType,
    Protocol,
    Tag(String),
}

impl Field {
    pub const BUILTIN: [Field; 5] = [
        Field::Context,
        Field::Namespace,
        Field::Kubeconfig,
        Field::WorkloadType,
        Field::Protocol,
    ];

    pub fn value<'a>(&self, config: &'a Config) -> Option<&'a str> {
        match self {
            Field::Context => config.context.as_deref().filter(|v| !v.is_empty()),
            Field::Namespace => Some(config.namespace.as_str()).filter(|v| !v.is_empty()),
            Field::Kubeconfig => config.kubeconfig.as_deref().filter(|v| !v.is_empty()),
            Field::WorkloadType => config.workload_type.as_deref().filter(|v| !v.is_empty()),
            Field::Protocol => Some(config.protocol.as_str()).filter(|v| !v.is_empty()),
            Field::Tag(key) => config.tags.get(key).map(String::as_str),
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Field::Context => f.write_str("context"),
            Field::Namespace => f.write_str("namespace"),
            Field::Kubeconfig => f.write_str("kubeconfig"),
            Field::WorkloadType => f.write_str("workload_type"),
            Field::Protocol => f.write_str("protocol"),
            Field::Tag(key) => write!(f, "{TAG_PREFIX}{key}"),
        }
    }
}

impl FromStr for Field {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "context" => Ok(Field::Context),
            "namespace" => Ok(Field::Namespace),
            "kubeconfig" => Ok(Field::Kubeconfig),
            "workload_type" => Ok(Field::WorkloadType),
            "protocol" => Ok(Field::Protocol),
            other => match other.strip_prefix(TAG_PREFIX) {
                Some(key) => {
                    validate_tag_key(key)?;
                    Ok(Field::Tag(key.to_string()))
                }
                None => Err(format!(
                    "unknown field '{other}', expected one of: context, namespace, kubeconfig, workload_type, protocol, tag:<key>"
                )),
            },
        }
    }
}

impl TryFrom<String> for Field {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Field> for String {
    fn from(field: Field) -> Self {
        field.to_string()
    }
}

/// A filter on one field. Empty `values` means "field is set".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub field: Field,
    #[serde(default)]
    pub values: BTreeSet<String>,
}

impl Condition {
    pub fn matches(&self, config: &Config) -> bool {
        self.field
            .value(config)
            .is_some_and(|value| self.values.is_empty() || self.values.contains(value))
    }
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.field)?;
        if !self.values.is_empty() {
            let values: Vec<&str> = self.values.iter().map(String::as_str).collect();
            write!(f, "={}", values.join(","))?;
        }
        Ok(())
    }
}

impl FromStr for Condition {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (field, values) = s.split_once('=').unwrap_or((s, ""));
        let values = values
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect();
        Ok(Condition {
            field: field.parse()?,
            values,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigView {
    pub group_by: Option<Field>,
    pub filters: Vec<Condition>,
}

impl Default for ConfigView {
    fn default() -> Self {
        Self {
            group_by: Some(Field::Context),
            filters: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigGroup {
    pub key: Option<String>,
    pub label: String,
    pub config_ids: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facet {
    pub field: Field,
    /// Configs where the field is set.
    pub count: usize,
    pub values: Vec<FacetValue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetValue {
    pub value: String,
    pub count: usize,
}

impl ConfigView {
    pub fn matches(&self, config: &Config) -> bool {
        self.filters
            .iter()
            .all(|condition| condition.matches(config))
    }

    fn condition_mut(&mut self, field: &Field) -> Option<&mut Condition> {
        self.filters.iter_mut().find(|c| c.field == *field)
    }

    /// Toggles `value` in the field's condition, replacing a "field is set"
    /// condition and dropping the condition once no values remain.
    pub fn toggle_value(&mut self, field: &Field, value: &str) {
        let Some(condition) = self.condition_mut(field) else {
            self.filters.push(Condition {
                field: field.clone(),
                values: BTreeSet::from([value.to_string()]),
            });
            return;
        };
        if !condition.values.remove(value) {
            condition.values.insert(value.to_string());
        } else if condition.values.is_empty() {
            self.filters.retain(|c| c.field != *field);
        }
    }

    /// Toggles a "field is set" condition, replacing any value condition.
    pub fn toggle_present(&mut self, field: &Field) {
        match self.condition_mut(field) {
            Some(condition) if condition.values.is_empty() => {
                self.filters.retain(|c| c.field != *field)
            }
            Some(condition) => condition.values.clear(),
            None => self.filters.push(Condition {
                field: field.clone(),
                values: BTreeSet::new(),
            }),
        }
    }

    pub fn is_present_filter(&self, field: &Field) -> bool {
        self.filters
            .iter()
            .any(|c| c.field == *field && c.values.is_empty())
    }

    pub fn is_value_filter(&self, field: &Field, value: &str) -> bool {
        self.filters
            .iter()
            .any(|c| c.field == *field && c.values.contains(value))
    }

    pub fn filter<'a>(&'a self, configs: &'a [Config]) -> impl Iterator<Item = &'a Config> {
        configs.iter().filter(|config| self.matches(config))
    }

    /// Filters and groups configs. Groups are sorted case-insensitively with
    /// the ungrouped bucket last; configs inside a group are sorted by alias.
    pub fn group(&self, configs: &[Config]) -> Vec<ConfigGroup> {
        let mut buckets: BTreeMap<Option<&str>, Vec<&Config>> = BTreeMap::new();
        for config in self.filter(configs) {
            let key = self.group_by.as_ref().and_then(|field| field.value(config));
            buckets.entry(key).or_default().push(config);
        }

        let mut groups: Vec<ConfigGroup> = buckets
            .into_iter()
            .map(|(key, mut members)| {
                members
                    .sort_by_cached_key(|c| c.alias.as_deref().unwrap_or_default().to_lowercase());
                ConfigGroup {
                    label: self.group_label(key),
                    key: key.map(str::to_string),
                    config_ids: members.iter().filter_map(|c| c.id).collect(),
                }
            })
            .collect();

        groups.sort_by_cached_key(|g| (g.key.is_none(), g.label.to_lowercase()));
        groups
    }

    fn group_label(&self, key: Option<&str>) -> String {
        match (key, &self.group_by) {
            (None, Some(_)) => UNGROUPED_LABEL.to_string(),
            (None, None) => "All".to_string(),
            (Some(""), Some(Field::Tag(tag))) => tag.clone(),
            (Some(value), _) => value.to_string(),
        }
    }
}

/// Distinct values per field, used to populate group-by and filter pickers.
pub fn facets(configs: &[Config]) -> Vec<Facet> {
    let tag_keys: BTreeSet<&String> = configs.iter().flat_map(|c| c.tags.keys()).collect();

    Field::BUILTIN
        .into_iter()
        .chain(tag_keys.into_iter().map(|key| Field::Tag(key.clone())))
        .map(|field| {
            let present: Vec<&str> = configs.iter().filter_map(|c| field.value(c)).collect();
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for value in present.iter().filter(|v| !v.is_empty()) {
                *counts.entry(value).or_default() += 1;
            }
            Facet {
                count: present.len(),
                values: counts
                    .into_iter()
                    .map(|(value, count)| FacetValue {
                        value: value.to_string(),
                        count,
                    })
                    .collect(),
                field,
            }
        })
        .collect()
}

pub fn validate_tag_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("tag key cannot be empty".to_string());
    }
    if key.len() > MAX_TAG_KEY_LEN {
        return Err(format!(
            "tag key '{key}' exceeds {MAX_TAG_KEY_LEN} characters"
        ));
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-' | '/'))
    {
        return Err(format!(
            "tag key '{key}' may only contain lowercase letters, digits, '.', '_', '-' and '/'"
        ));
    }
    Ok(())
}

pub fn validate_tag_value(value: &str) -> Result<(), String> {
    if value.len() > MAX_TAG_VALUE_LEN {
        return Err(format!(
            "tag value '{value}' exceeds {MAX_TAG_VALUE_LEN} characters"
        ));
    }
    if value.contains([',', '=']) {
        return Err(format!("tag value '{value}' cannot contain ',' or '='"));
    }
    Ok(())
}

pub fn validate_tags(tags: &BTreeMap<String, String>) -> Result<(), String> {
    tags.iter().try_for_each(|(key, value)| {
        validate_tag_key(key)?;
        validate_tag_value(value)
    })
}

/// Trims values and lowercases keys, dropping entries with an empty key.
pub fn normalize_tags(tags: BTreeMap<String, String>) -> BTreeMap<String, String> {
    tags.into_iter()
        .map(|(key, value)| (key.trim().to_lowercase(), value.trim().to_string()))
        .filter(|(key, _)| !key.is_empty())
        .collect()
}

/// Parses `key=value, other, k2=v2` into a normalized, validated tag map.
pub fn parse_tags(input: &str) -> Result<BTreeMap<String, String>, String> {
    let tags = input
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (key, value) = entry.split_once('=').unwrap_or((entry, ""));
            (key.to_string(), value.to_string())
        })
        .collect();
    let tags = normalize_tags(tags);
    validate_tags(&tags)?;
    Ok(tags)
}

pub fn format_tags(tags: &BTreeMap<String, String>) -> String {
    tags.iter()
        .map(|(key, value)| {
            if value.is_empty() {
                key.clone()
            } else {
                format!("{key}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub async fn get_config_view_with_mode(mode: DatabaseMode) -> ConfigView {
    match get_setting_with_mode(CONFIG_VIEW_KEY, mode).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            warn!("Invalid stored config view, using default: {e}");
            ConfigView::default()
        }),
        Ok(None) => ConfigView::default(),
        Err(e) => {
            warn!("Failed to read config view, using default: {e}");
            ConfigView::default()
        }
    }
}

pub async fn set_config_view_with_mode(
    view: &ConfigView, mode: DatabaseMode,
) -> Result<(), String> {
    let raw = serde_json::to_string(view).map_err(|e| e.to_string())?;
    set_setting_with_mode(CONFIG_VIEW_KEY, &raw, mode)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(id: i64, alias: &str, context: &str, tags: &[(&str, &str)]) -> Config {
        Config {
            id: Some(id),
            alias: Some(alias.to_string()),
            context: Some(context.to_string()),
            namespace: "default".to_string(),
            tags: tags
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Config::default()
        }
    }

    fn sample() -> Vec<Config> {
        vec![
            config(1, "b-api", "prod", &[("team", "payments"), ("env", "prod")]),
            config(2, "a-db", "prod", &[("team", "core")]),
            config(3, "c-web", "Dev", &[("pinned", "")]),
            config(4, "d-cache", "dev", &[]),
        ]
    }

    #[test]
    fn field_round_trips() {
        for raw in [
            "context",
            "namespace",
            "kubeconfig",
            "workload_type",
            "protocol",
            "tag:team",
        ] {
            assert_eq!(raw.parse::<Field>().unwrap().to_string(), raw);
        }
        assert!("tag:".parse::<Field>().is_err());
        assert!("tag:Team".parse::<Field>().is_err());
        assert!("alias".parse::<Field>().is_err());
    }

    #[test]
    fn condition_round_trips() {
        let condition: Condition = "tag:team= payments , core".parse().unwrap();
        assert_eq!(condition.field, Field::Tag("team".into()));
        assert_eq!(condition.to_string(), "tag:team=core,payments");
        assert_eq!(
            "tag:pinned".parse::<Condition>().unwrap().to_string(),
            "tag:pinned"
        );
    }

    #[test]
    fn filters_and_across_conditions_or_within() {
        let configs = sample();
        let view = ConfigView {
            group_by: None,
            filters: vec![
                "context=prod,dev".parse().unwrap(),
                "tag:team".parse().unwrap(),
            ],
        };
        let ids: Vec<_> = view.filter(&configs).filter_map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2]);

        let pinned = ConfigView {
            group_by: None,
            filters: vec!["tag:pinned".parse().unwrap()],
        };
        let ids: Vec<_> = pinned.filter(&configs).filter_map(|c| c.id).collect();
        assert_eq!(ids, vec![3]);
    }

    #[test]
    fn groups_sorted_with_ungrouped_last() {
        let configs = sample();
        let view = ConfigView {
            group_by: Some(Field::Tag("team".into())),
            filters: vec![],
        };
        let groups = view.group(&configs);
        let summary: Vec<_> = groups
            .iter()
            .map(|g| (g.key.as_deref(), g.label.as_str(), g.config_ids.clone()))
            .collect();
        assert_eq!(
            summary,
            vec![
                (Some("core"), "core", vec![2]),
                (Some("payments"), "payments", vec![1]),
                (None, UNGROUPED_LABEL, vec![3, 4]),
            ]
        );
    }

    #[test]
    fn group_keys_are_case_sensitive_but_sorted_case_insensitively() {
        let groups = ConfigView::default().group(&sample());
        let keys: Vec<_> = groups.iter().map(|g| g.key.as_deref()).collect();
        assert_eq!(keys, vec![Some("Dev"), Some("dev"), Some("prod")]);
        assert_eq!(groups[2].config_ids, vec![2, 1]);
    }

    #[test]
    fn valueless_tag_group_uses_tag_key_as_label() {
        let view = ConfigView {
            group_by: Some(Field::Tag("pinned".into())),
            filters: vec![],
        };
        let groups = view.group(&sample());
        assert_eq!(groups[0].key.as_deref(), Some(""));
        assert_eq!(groups[0].label, "pinned");
    }

    #[test]
    fn no_group_by_yields_single_group() {
        let view = ConfigView {
            group_by: None,
            filters: vec![],
        };
        let groups = view.group(&sample());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].config_ids, vec![2, 1, 3, 4]);
    }

    #[test]
    fn facets_include_tag_keys() {
        let facets = facets(&sample());
        let team = facets
            .iter()
            .find(|f| f.field == Field::Tag("team".into()))
            .unwrap();
        let team_values: Vec<_> = team
            .values
            .iter()
            .map(|v| (v.value.as_str(), v.count))
            .collect();
        assert_eq!(team_values, vec![("core", 1), ("payments", 1)]);
        assert_eq!(team.count, 2);
        let pinned = facets
            .iter()
            .find(|f| f.field == Field::Tag("pinned".into()))
            .unwrap();
        assert!(pinned.values.is_empty());
        assert_eq!(pinned.count, 1);
    }

    #[test]
    fn view_serde_defaults_and_null_group_by() {
        assert_eq!(
            serde_json::from_str::<ConfigView>("{}").unwrap(),
            ConfigView::default()
        );
        let view: ConfigView = serde_json::from_str(
            r#"{"group_by":null,"filters":[{"field":"tag:env","values":["dev"]}]}"#,
        )
        .unwrap();
        assert_eq!(view.group_by, None);
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            r#"{"group_by":null,"filters":[{"field":"tag:env","values":["dev"]}]}"#
        );
        assert!(serde_json::from_str::<ConfigView>(r#"{"group_by":"bogus"}"#).is_err());
    }

    #[test]
    fn toggles_keep_one_condition_per_field() {
        let team = Field::Tag("team".into());
        let mut view = ConfigView::default();

        view.toggle_value(&team, "core");
        view.toggle_value(&team, "payments");
        assert_eq!(view.filters.len(), 1);
        assert!(view.is_value_filter(&team, "core"));

        view.toggle_present(&team);
        assert_eq!(view.filters.len(), 1);
        assert!(view.is_present_filter(&team));

        view.toggle_value(&team, "core");
        assert!(!view.is_present_filter(&team));
        assert!(view.is_value_filter(&team, "core"));

        view.toggle_value(&team, "core");
        assert!(view.filters.is_empty());

        view.toggle_present(&team);
        view.toggle_present(&team);
        assert!(view.filters.is_empty());
    }

    #[test]
    fn parse_and_format_tags() {
        let tags = parse_tags(" Team=payments, pinned ,, env = prod ").unwrap();
        assert_eq!(format_tags(&tags), "env=prod, pinned, team=payments");
        assert!(parse_tags("bad key=x").is_err());
        assert!(parse_tags("k=a=b").is_err());
    }
}
