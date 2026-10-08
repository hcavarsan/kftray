use thirtyfour::By;

#[derive(Clone, Copy, Debug)]
pub enum TestId {
    MainView,
    ConsentDialog,
    ConsentAllow,
    ConsentDecline,
    AddConfigButton,
    ExpandGroups,
    ConfigRow,
    ConfigToggle,
    ConfigMenu,
    SettingsButton,
    SettingsDialog,
    SettingsTelemetry,
}

impl TestId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MainView => "main-view",
            Self::ConsentDialog => "consent-dialog",
            Self::ConsentAllow => "consent-allow",
            Self::ConsentDecline => "consent-decline",
            Self::AddConfigButton => "add-config-button",
            Self::ExpandGroups => "expand-groups",
            Self::ConfigRow => "config-row",
            Self::ConfigToggle => "config-toggle",
            Self::ConfigMenu => "config-menu",
            Self::SettingsButton => "settings-button",
            Self::SettingsDialog => "settings-dialog",
            Self::SettingsTelemetry => "settings-telemetry",
        }
    }

    pub fn css(self) -> String {
        format!("[data-testid='{}']", self.as_str())
    }

    pub fn by(self) -> By {
        By::Css(self.css())
    }
}
