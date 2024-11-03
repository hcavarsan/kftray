use std::fs::OpenOptions;
use std::io::{
    BufRead,
    BufReader,
    Write,
};
use std::path::PathBuf;

use crate::db::operations::Database;
use crate::error::Result;

pub struct HostsBuilder<'a> {
    comment: &'a str,
}

impl<'a> HostsBuilder<'a> {
    pub fn new(comment: &'a str) -> Self {
        Self { comment }
    }

    pub fn write(&self) -> Result<()> {
        let hosts_path = get_hosts_path()?;
        let file = OpenOptions::new().read(true).open(&hosts_path)?;
        let reader = BufReader::new(file);

        let lines: Vec<String> = reader
            .lines()
            .filter_map(|line| line.ok())
            .filter(|line| !line.contains(self.comment))
            .collect();

        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(hosts_path)?;

        for line in lines {
            writeln!(file, "{}", line)?;
        }

        Ok(())
    }
}

fn get_hosts_path() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        Ok(PathBuf::from(r"C:\Windows\System32\drivers\etc\hosts"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(PathBuf::from("/etc/hosts"))
    }
}

pub async fn clean_all_custom_hosts_entries(database: &Database) -> Result<()> {
    let configs = database.get_all_configs().await?;

    for config in configs {
        let hostfile_comment = format!(
            "kftray custom host for {} - {}",
            config.service.unwrap_or_default(),
            config.id.unwrap_or_default()
        );

        let hosts_builder = HostsBuilder::new(&hostfile_comment);
        hosts_builder.write()?;
    }

    Ok(())
}
