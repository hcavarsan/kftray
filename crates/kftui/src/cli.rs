use clap::Parser;

#[derive(Parser)]
#[command(name = "kftui")]
#[command(about = "KFtray TUI - Manage kubectl port forward configurations")]
#[command(version)]
pub struct Cli {
    #[arg(
        short = 'c',
        long,
        help = "Path to config file (local JSON file or path within GitHub repo)",
        value_name = "PATH"
    )]
    pub configs_path: Option<String>,

    #[arg(
        short = 'g',
        long,
        help = "GitHub repository URL to import configs from",
        value_name = "URL"
    )]
    pub github_url: Option<String>,

    #[arg(
        short = 's',
        long,
        help = "Save configs to SQLite database (requires --configs-path or --github-url)"
    )]
    pub save: bool,

    #[arg(short = 'a', long, help = "Auto-start all port forward configurations")]
    pub auto_start: bool,

    #[arg(long, help = "Clear existing configurations before importing")]
    pub flush: bool,
}

impl Cli {
    pub fn should_use_memory_mode(&self) -> bool {
        (self.configs_path.is_some() || self.github_url.is_some()) && !self.save
    }

    pub fn is_github_import(&self) -> bool {
        self.github_url.is_some()
    }

    pub fn has_config_source(&self) -> bool {
        self.configs_path.is_some() || self.github_url.is_some()
    }

    pub fn get_config_path(&self) -> Option<&str> {
        self.configs_path.as_deref()
    }

    pub fn get_github_url(&self) -> Option<&str> {
        self.github_url.as_deref()
    }

    pub fn get_configs_path_with_default(&self) -> String {
        self.configs_path
            .clone()
            .unwrap_or_else(|| "config.json".to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.save && !self.has_config_source() {
            return Err("--save requires either --configs-path or --github-url".to_string());
        }

        if self.github_url.is_some() && self.configs_path.is_none() {
            return Err("--github-url requires --configs-path to specify the config file path within the repository".to_string());
        }

        Ok(())
    }
}
