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

    #[arg(
        short = 'j',
        long,
        help = "Inline JSON configuration string",
        value_name = "JSON"
    )]
    pub json: Option<String>,

    #[arg(long, help = "Read JSON configuration from stdin")]
    pub stdin: bool,
}

impl Cli {
    pub fn should_use_memory_mode(&self) -> bool {
        (self.configs_path.is_some()
            || self.github_url.is_some()
            || self.json.is_some()
            || self.stdin)
            && !self.save
    }

    pub fn is_github_import(&self) -> bool {
        self.github_url.is_some()
    }

    pub fn has_config_source(&self) -> bool {
        self.configs_path.is_some()
            || self.github_url.is_some()
            || self.json.is_some()
            || self.stdin
    }

    pub fn get_config_path(&self) -> Option<&str> {
        self.configs_path.as_deref()
    }

    pub fn get_github_url(&self) -> Option<&str> {
        self.github_url.as_deref()
    }

    pub fn get_json(&self) -> Option<&str> {
        self.json.as_deref()
    }

    pub fn get_configs_path_with_default(&self) -> String {
        self.configs_path
            .clone()
            .unwrap_or_else(|| "config.json".to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.save && !self.has_config_source() {
            return Err(
                "--save requires either --configs-path, --github-url, --json, or --stdin"
                    .to_string(),
            );
        }

        let mut source_count = 0;

        if self.configs_path.is_some() && self.github_url.is_none() {
            source_count += 1;
        }

        if self.github_url.is_some() {
            source_count += 1;
            if self.configs_path.is_none() {
                return Err("--github-url requires --configs-path to specify the config file path within the repository".to_string());
            }
        }

        if self.json.is_some() {
            source_count += 1;
        }

        if self.stdin {
            source_count += 1;
        }

        if source_count > 1 {
            return Err(
                "Only one config source can be specified: --configs-path, --github-url (with --configs-path), --json, or --stdin"
                    .to_string(),
            );
        }

        Ok(())
    }
}
