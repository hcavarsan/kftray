use thirtyfour::By;

#[derive(Clone, Copy, Debug)]
pub enum TestId {
    MainView,
    ConsentDialog,
    ConsentOption,
    ConsentSave,
    ConsentDecline,
    AddConfigButton,
    ExpandGroups,
    ConfigRow,
    ConfigToggle,
    ConfigError,
    ConfigMenu,
    MenuImport,
    MenuExport,
    SettingsButton,
    SettingsDialog,
    SettingsTelemetry,
    SettingsSave,
    AddConfigDialog,
    ConfigAlias,
    ConfigContext,
    ConfigNamespace,
    ConfigService,
    ConfigLocalPort,
    ConfigRemotePort,
    ConfigWorkloadType,
    ConfigProtocol,
    ConfigSave,
}

impl TestId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MainView => "main-view",
            Self::ConsentDialog => "consent-dialog",
            Self::ConsentOption => "consent-option",
            Self::ConsentSave => "consent-save",
            Self::ConsentDecline => "consent-decline",
            Self::AddConfigButton => "add-config-button",
            Self::ExpandGroups => "expand-groups",
            Self::ConfigRow => "config-row",
            Self::ConfigToggle => "config-toggle",
            Self::ConfigError => "config-error",
            Self::ConfigMenu => "config-menu",
            Self::MenuImport => "menu-import",
            Self::MenuExport => "menu-export",
            Self::SettingsButton => "settings-button",
            Self::SettingsDialog => "settings-dialog",
            Self::SettingsTelemetry => "settings-telemetry",
            Self::SettingsSave => "settings-save",
            Self::AddConfigDialog => "add-config-dialog",
            Self::ConfigAlias => "config-alias",
            Self::ConfigContext => "config-context",
            Self::ConfigNamespace => "config-namespace",
            Self::ConfigService => "config-service",
            Self::ConfigLocalPort => "config-local-port",
            Self::ConfigRemotePort => "config-remote-port",
            Self::ConfigWorkloadType => "config-workload-type",
            Self::ConfigProtocol => "config-protocol",
            Self::ConfigSave => "config-save",
        }
    }

    pub fn css(self) -> String {
        format!("[data-testid='{}']", self.as_str())
    }

    pub fn by(self) -> By {
        By::Css(self.css())
    }
}
