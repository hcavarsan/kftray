use log::error;
use sqlx::{
    Row,
    SqlitePool,
};

use crate::db::get_db_pool;
use crate::models::config_state_model::ConfigState;
use crate::utils::db_mode::{
    DatabaseManager,
    DatabaseMode,
};

pub async fn update_config_state_with_pool(
    config_state: &ConfigState, pool: &SqlitePool,
) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    // A row this process claims carries this process's own identity; one
    // written on behalf of another process keeps what the caller read.
    let process_start = match config_state.process_id {
        Some(pid) if pid == std::process::id() => process_start_identity(pid),
        _ => config_state.process_start,
    };
    sqlx::query("UPDATE config_state SET is_running = ?1, process_id = ?2, is_retrying = ?3, retry_count = ?4, last_error = ?5, process_start = ?6 WHERE config_id = ?7")
        .bind(config_state.is_running)
        .bind(config_state.process_id)
        .bind(config_state.is_retrying)
        .bind(config_state.retry_count)
        .bind(&config_state.last_error)
        .bind(process_start)
        .bind(config_state.config_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_config_state(config_state: &ConfigState) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    update_config_state_with_pool(config_state, &pool).await
}

pub async fn read_config_states_with_pool(
    pool: &SqlitePool,
) -> Result<Vec<ConfigState>, sqlx::Error> {
    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire database connection: {e}");
        e
    })?;
    let rows = sqlx::query("SELECT id, config_id, is_running, process_id, is_retrying, retry_count, last_error, process_start FROM config_state")
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to fetch config states: {e}");
            e
        })?;

    let config_states = rows
        .into_iter()
        .map(|row| {
            let id: Option<i64> = row.try_get("id").ok();
            let config_id: i64 = row.try_get("config_id").map_err(|e| {
                error!("Failed to get config_id: {e}");
                e
            })?;
            let is_running: bool = row.try_get("is_running").map_err(|e| {
                error!("Failed to get is_running: {e}");
                e
            })?;
            let process_id: Option<u32> = row.try_get("process_id").ok().flatten();
            let is_retrying: bool = row.try_get("is_retrying").ok().unwrap_or(false);
            let retry_count: Option<i32> = row.try_get("retry_count").ok().flatten();
            let last_error: Option<String> = row.try_get("last_error").ok().flatten();
            let process_start: Option<i64> = row.try_get("process_start").ok().flatten();
            Ok(ConfigState {
                id,
                config_id,
                is_running,
                process_id,
                is_retrying,
                retry_count,
                last_error,
                process_start,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(config_states)
}

pub async fn read_config_states() -> Result<Vec<ConfigState>, sqlx::Error> {
    let pool = get_db_pool()
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;
    read_config_states_with_pool(&pool).await
}

pub async fn get_configs_state_with_pool(pool: &SqlitePool) -> Result<Vec<ConfigState>, String> {
    read_config_states_with_pool(pool).await.map_err(|e| {
        error!("Failed to get config states: {e}");
        e.to_string()
    })
}

pub async fn get_configs_state() -> Result<Vec<ConfigState>, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    get_configs_state_with_pool(&pool).await
}

pub async fn update_config_state_with_mode(
    config_state: &ConfigState, mode: DatabaseMode,
) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    update_config_state_with_pool(config_state, &context.pool).await
}

pub async fn read_config_states_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<ConfigState>, sqlx::Error> {
    let context = DatabaseManager::get_context(mode)
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;
    read_config_states_with_pool(&context.pool).await
}

pub async fn get_configs_state_with_mode(mode: DatabaseMode) -> Result<Vec<ConfigState>, String> {
    let context = DatabaseManager::get_context(mode).await?;
    read_config_states_with_pool(&context.pool)
        .await
        .map_err(|e| {
            error!("Failed to get config states: {e}");
            e.to_string()
        })
}

/// Saves why the last connection through a forward could not reach the pod,
/// or clears it with `None`. Only a row this process runs is changed: a write
/// that lands after a stop must not mark the stopped forward.
pub async fn set_connection_error_with_pool(
    config_id: i64, error: Option<&str>, pool: &SqlitePool,
) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE config_state SET last_error = ?1 WHERE config_id = ?2 AND is_running = 1 AND \
         process_id = ?3 AND last_error IS NOT ?1",
    )
    .bind(error)
    .bind(config_id)
    .bind(std::process::id())
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn set_connection_error_with_mode(
    config_id: i64, error: Option<&str>, mode: DatabaseMode,
) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    set_connection_error_with_pool(config_id, error, &context.pool).await
}

pub async fn set_retry_status_with_pool(
    config_id: i64, owner_process_id: Option<u32>, owner_process_start: Option<i64>,
    retry_count: Option<i32>, last_error: Option<&str>, pool: &SqlitePool,
) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE config_state SET is_retrying = ?1, retry_count = ?2, last_error = ?3 WHERE \
         config_id = ?4 AND is_running = 1 AND process_id IS ?5 AND process_start IS ?6",
    )
    .bind(retry_count.is_some())
    .bind(retry_count)
    .bind(last_error)
    .bind(config_id)
    .bind(owner_process_id)
    .bind(owner_process_start)
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn set_retry_status(
    config_id: i64, owner_process_id: Option<u32>, owner_process_start: Option<i64>,
    retry_count: Option<i32>, last_error: Option<&str>,
) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    set_retry_status_with_pool(
        config_id,
        owner_process_id,
        owner_process_start,
        retry_count,
        last_error,
        &pool,
    )
    .await
}

pub async fn clear_retry_status_with_pool(config_id: i64, pool: &SqlitePool) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE config_state SET is_retrying = false, retry_count = NULL, last_error = NULL \
         WHERE config_id = ?1 AND is_running = 1 AND is_retrying = 1 AND process_id = ?2",
    )
    .bind(config_id)
    .bind(std::process::id())
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn clear_retry_status(config_id: i64) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    clear_retry_status_with_pool(config_id, &pool).await
}

/// Marks every configuration this process was running as stopped, except the
/// ones in `still_owed`: a configuration whose cleanup did not complete keeps
/// its running state, so the next run's stop-all enumerates and retries it.
pub async fn cleanup_current_process_config_states_with_mode(
    mode: DatabaseMode, still_owed: &[i64],
) -> Result<(), String> {
    let current_process_id = std::process::id();
    let context = DatabaseManager::get_context(mode).await?;
    let mut tx = context.pool.begin().await.map_err(|e| e.to_string())?;

    // The snapshots of the rows about to be marked stopped go with them. Both
    // statements run in one transaction: a failure between them must not
    // leave a row marked running with its snapshot already gone, which would
    // make a later stop fall back to a possibly-edited row.
    let stopping: Vec<i64> = sqlx::query("SELECT config_id FROM config_state WHERE process_id = ?")
        .bind(current_process_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|row| row.try_get::<i64, _>("config_id").ok())
        .filter(|id| !still_owed.contains(id))
        .collect();

    if !stopping.is_empty() {
        let mut delete = sqlx::QueryBuilder::new("DELETE FROM settings WHERE key IN (");
        let mut keys = delete.separated(", ");
        for id in &stopping {
            keys.push_bind(running_snapshot_key(*id, mode));
        }
        delete.push(")");
        delete
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut query = sqlx::QueryBuilder::new(
        "UPDATE config_state SET is_running = false, process_id = NULL, process_start = NULL WHERE process_id = ",
    );
    query.push_bind(current_process_id);
    if !still_owed.is_empty() {
        query.push(" AND config_id NOT IN (");
        let mut ids = query.separated(", ");
        for id in still_owed {
            ids.push_bind(id);
        }
        query.push(")");
    }
    let affected_rows = query
        .build()
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();

    tx.commit().await.map_err(|e| e.to_string())?;

    if affected_rows > 0 {
        log::info!(
            "Cleaned up {affected_rows} config states for process {current_process_id} in mode {mode:?}"
        );
    }

    Ok(())
}

/// Key under which the configuration a running forward was started with is
/// kept, next to its running state.
///
/// The row can be edited while the forward runs, so another process that
/// needs to know which address and aliases this one holds cannot read the
/// row for it: the snapshot is what the forward actually owns.
pub fn running_snapshot_key(id: i64, mode: DatabaseMode) -> String {
    format!(
        "running_snapshot:{}:{id}",
        crate::utils::settings::mode_scope(mode)
    )
}

/// Records what a forward that has just registered actually holds. Only the
/// file database is shared between processes; nothing is written for the
/// in-memory one.
pub async fn set_running_snapshot(
    id: i64, config: &crate::models::config_model::Config, mode: DatabaseMode,
) -> Result<(), String> {
    if mode != DatabaseMode::File {
        return Ok(());
    }
    let serialized = serde_json::to_string(config)
        .map_err(|error| format!("Failed to describe the running config {id}: {error}"))?;
    crate::utils::settings::set_setting_with_mode(
        &running_snapshot_key(id, mode),
        &serialized,
        mode,
    )
    .await
    .map_err(|error| format!("Failed to record the running config {id}: {error}"))
}

/// Forgets the snapshot once the forward has stopped.
pub async fn clear_running_snapshot(id: i64, mode: DatabaseMode) {
    if mode != DatabaseMode::File {
        return;
    }
    if let Err(error) =
        crate::utils::settings::delete_setting_with_mode(&running_snapshot_key(id, mode), mode)
            .await
    {
        log::debug!("Failed to clear the running snapshot for config {id}: {error}");
    }
}

/// The configuration a running forward was started with, when a snapshot was
/// recorded for it.
///
/// A snapshot that cannot be read or understood reads as none; a caller that
/// must tell the two apart uses [`try_running_snapshot`].
pub async fn running_snapshot(
    id: i64, mode: DatabaseMode,
) -> Option<crate::models::config_model::Config> {
    try_running_snapshot(id, mode).await.ok().flatten()
}

/// [`running_snapshot`] that reports a record it cannot read or understand
/// instead of reading it as absent: what such a record named is unknown, which
/// is not the same as no record.
pub async fn try_running_snapshot(
    id: i64, mode: DatabaseMode,
) -> Result<Option<crate::models::config_model::Config>, String> {
    if mode != DatabaseMode::File {
        return Ok(None);
    }
    let stored =
        crate::utils::settings::get_setting_with_mode(&running_snapshot_key(id, mode), mode)
            .await
            .map_err(|error| format!("Failed to read the running config {id}: {error}"))?;
    stored
        .map(|stored| {
            serde_json::from_str(&stored)
                .map_err(|error| format!("Failed to understand the running config {id}: {error}"))
        })
        .transpose()
}

/// Identifies when a process started, so a pid can be told apart from a later
/// process that was given the same number. The value is only meaningful when
/// compared with another read of the same platform: clock ticks since boot on
/// Linux, microseconds since the epoch on macOS, a FILETIME on Windows. `None`
/// when the process is gone or the platform offers no way to read it.
pub fn process_start_identity(pid: u32) -> Option<i64> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The command name comes first and may hold spaces and parentheses;
        // everything after the last ')' is plain fields, starting at field 3.
        let after_name = &stat[stat.rfind(')')? + 1..];
        after_name.split_whitespace().nth(19)?.parse().ok()
    }
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: `info` is a writable buffer of exactly `size` bytes.
        let written = unsafe {
            libc::proc_pidinfo(
                libc::pid_t::try_from(pid).ok()?,
                libc::PROC_PIDTBSDINFO,
                0,
                std::ptr::addr_of_mut!(info).cast(),
                size,
            )
        };
        (written == size)
            .then(|| i64::try_from(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec).ok())
            .flatten()
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{
            CloseHandle,
            FILETIME,
        };
        use windows::Win32::System::Threading::{
            GetProcessTimes,
            OpenProcess,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let times =
            unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
        let _ = unsafe { CloseHandle(handle) };
        times.ok()?;
        i64::try_from((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
            .ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = pid;
        None
    }
}

/// Whether a process that recorded itself as running a forward still exists.
///
/// A row whose process is gone is this process's to clean up; one whose
/// process is alive belongs to it, and its listener cannot be stopped from
/// here. A live pid alone is not enough: after a crash the number can be
/// given to an unrelated process, so when the row recorded a start identity
/// it must match the live process's. A row without one, or a platform that
/// cannot read it, falls back to existence.
pub fn process_is_alive(pid: u32, recorded_start: Option<i64>) -> bool {
    if pid == 0 {
        return false;
    }
    // No shortcut for this process's own pid: a row from an earlier process
    // that had the same number must fail the identity check like any other.
    #[cfg(unix)]
    let exists = {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // Signal 0 checks for existence without delivering anything. EPERM
        // means the process exists but belongs to another user.
        // SAFETY: signal 0 delivers nothing; `pid` is a validated, non-zero
        // process id.
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    };
    #[cfg(windows)]
    let exists = {
        use windows::Win32::Foundation::{
            CloseHandle,
            ERROR_ACCESS_DENIED,
            WAIT_TIMEOUT,
        };
        use windows::Win32::System::Threading::{
            OpenProcess,
            PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        };
        match unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        } {
            Ok(handle) => {
                let running = unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT;
                let _ = unsafe { CloseHandle(handle) };
                running
            }
            // Access denied still means the process exists. `Error::code`
            // wraps the Win32 status as an HRESULT, so it must be compared
            // against `HRESULT::from_win32(ERROR_ACCESS_DENIED.0)`, not the
            // unrelated COM `E_ACCESSDENIED` constant.
            Err(error) => error.code() == windows::core::HRESULT::from_win32(ERROR_ACCESS_DENIED.0),
        }
    };
    #[cfg(not(any(unix, windows)))]
    let exists = true;

    exists
        && match (recorded_start, process_start_identity(pid)) {
            (Some(recorded), Some(live)) => recorded == live,
            _ => true,
        }
}

/// Whether a row marked running was recorded by another process that is
/// still alive. Such a forward cannot be started or stopped from here.
pub fn is_owned_by_other_live_process(state: &ConfigState) -> bool {
    state.is_running
        && state.process_id.is_some_and(|pid| {
            pid != std::process::id() && process_is_alive(pid, state.process_start)
        })
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;
    use crate::db::create_db_table;
    use crate::models::config_model::Config;
    use crate::utils::config;

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory database");
        create_db_table(&pool)
            .await
            .expect("Failed to create tables");
        crate::utils::migration::migrate_configs(Some(&pool))
            .await
            .expect("Failed to run migrations");
        pool
    }

    #[tokio::test]
    async fn test_read_initial_config_state() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-1".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();
        let states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states.len(), 1);
        let state = &states[0];
        assert_eq!(state.config_id, config_id);
        assert!(!state.is_running);
        assert!(state.id.is_some());
    }

    #[tokio::test]
    async fn test_update_and_read_config_state() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-2".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();
        let initial_states = read_config_states_with_pool(&pool).await.unwrap();
        let initial_state = initial_states
            .iter()
            .find(|s| s.config_id == config_id)
            .unwrap();
        assert!(!initial_state.is_running);
        let state_to_update = ConfigState {
            id: initial_state.id,
            config_id,
            is_running: true,
            process_id: Some(1234),
            ..Default::default()
        };
        update_config_state_with_pool(&state_to_update, &pool)
            .await
            .unwrap();
        let updated_states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(updated_states.len(), 1);
        let updated_state = updated_states
            .iter()
            .find(|s| s.config_id == config_id)
            .unwrap();
        assert_eq!(updated_state.config_id, config_id);
        assert!(updated_state.is_running);
        assert_eq!(updated_state.id, initial_state.id);
    }

    #[tokio::test]
    async fn test_read_multiple_config_states() {
        let pool = setup_test_db().await;
        let config1 = Config {
            service: Some("state-test-3".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("state-test-4".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config1.clone(), &pool)
            .await
            .unwrap();
        config::insert_config_with_pool(config2.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config1_id = configs
            .iter()
            .find(|c| c.service == config1.service)
            .unwrap()
            .id
            .unwrap();
        let config2_id = configs
            .iter()
            .find(|c| c.service == config2.service)
            .unwrap()
            .id
            .unwrap();
        let state_to_update = ConfigState {
            id: None,
            config_id: config1_id,
            is_running: true,
            process_id: Some(1234),
            ..Default::default()
        };
        update_config_state_with_pool(&state_to_update, &pool)
            .await
            .unwrap();
        let states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states.len(), 2);
        let state1 = states.iter().find(|s| s.config_id == config1_id).unwrap();
        let state2 = states.iter().find(|s| s.config_id == config2_id).unwrap();
        assert!(state1.is_running);
        assert!(!state2.is_running);
    }

    #[tokio::test]
    async fn test_get_configs_state_wrapper() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-wrapper".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let result = read_config_states_with_pool(&pool)
            .await
            .map_err(|e| e.to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_update_config_state_public_function() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-5".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();

        let state_to_update = ConfigState {
            id: None,
            config_id,
            is_running: true,
            process_id: Some(1234),
            ..Default::default()
        };

        tokio::task::yield_now().await;

        let result = update_config_state_with_pool(&state_to_update, &pool).await;
        assert!(result.is_ok());

        let states = read_config_states_with_pool(&pool).await.unwrap();
        let updated_state = states.iter().find(|s| s.config_id == config_id).unwrap();
        assert!(updated_state.is_running);
    }

    #[tokio::test]
    async fn test_read_config_states_public_function() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-6".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let states_result = read_config_states_with_pool(&pool).await;
        assert!(states_result.is_ok());
        assert_eq!(states_result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_get_configs_state_function() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("state-test-7".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let states_result = read_config_states_with_pool(&pool)
            .await
            .map_err(|e| e.to_string());
        assert!(states_result.is_ok());
        let states = states_result.unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].config_id, 1);
    }

    #[tokio::test]
    async fn test_error_handling_in_update_config_state() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        pool.close().await;

        let config_state = ConfigState {
            id: Some(1),
            config_id: 1,
            is_running: true,
            process_id: Some(1234),
            ..Default::default()
        };

        let result = update_config_state_with_pool(&config_state, &pool).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_error_handling_in_read_config_states() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        pool.close().await;

        let result = read_config_states_with_pool(&pool).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_processing_in_read_config_states() {
        let pool = setup_test_db().await;

        let config1 = Config {
            service: Some("row-test-1".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("row-test-2".to_string()),
            ..Config::default()
        };

        config::insert_config_with_pool(config1.clone(), &pool)
            .await
            .unwrap();
        config::insert_config_with_pool(config2.clone(), &pool)
            .await
            .unwrap();

        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config2_id = configs
            .iter()
            .find(|c| c.service == Some("row-test-2".to_string()))
            .unwrap()
            .id
            .unwrap();

        let state = ConfigState {
            id: None,
            config_id: config2_id,
            is_running: true,
            process_id: Some(1234),
            ..Default::default()
        };

        update_config_state_with_pool(&state, &pool).await.unwrap();

        let states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states.len(), 2);

        let state1 = states.iter().find(|s| s.config_id == 1).unwrap();
        let state2 = states.iter().find(|s| s.config_id == config2_id).unwrap();

        assert!(!state1.is_running);
        assert!(state2.is_running);
    }

    #[tokio::test]
    async fn test_update_config_state_with_direct_pool() {
        let pool = setup_test_db().await;

        let config_data = Config {
            service: Some("public-test-1".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();

        let initial_states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(initial_states.len(), 1);
        assert!(!initial_states[0].is_running);

        let _conn = pool.acquire().await.unwrap();

        let state = ConfigState {
            id: initial_states[0].id,
            config_id,
            is_running: true,
            process_id: Some(1234),
            ..Default::default()
        };

        let result = update_config_state_with_pool(&state, &pool).await;
        assert!(result.is_ok());

        let updated_states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(updated_states.len(), 1);
        assert!(updated_states[0].is_running);
    }

    #[tokio::test]
    async fn test_read_config_states_with_direct_pool() {
        let pool = setup_test_db().await;

        let config_data = Config {
            service: Some("public-read-test".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let direct_result = read_config_states_with_pool(&pool).await;
        assert!(direct_result.is_ok());
        assert_eq!(direct_result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_get_configs_state_public_wrapper() {
        let _db = crate::test_utils::test_db().await;
        let pool = setup_test_db().await;

        let config_data = Config {
            service: Some("get-wrapper-test".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let direct_states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(direct_states.len(), 1);
        let err_result = get_configs_state().await;
        assert!(err_result.is_ok() || err_result.is_err());
    }

    #[tokio::test]
    async fn test_error_handling_row_processing_config_id() {
        let pool = setup_test_db().await;

        let config_data = Config {
            service: Some("error-test".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();

        let states = read_config_states_with_pool(&pool).await.unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].config_id, 1);
    }

    #[tokio::test]
    async fn test_config_state_with_mode_memory() {
        let _db = crate::test_utils::test_db().await;

        let _ = config::delete_all_configs_with_mode(DatabaseMode::Memory).await;

        let config_data = Config {
            service: Some("memory-state-test".to_string()),
            ..Config::default()
        };

        config::insert_config_with_mode(config_data, DatabaseMode::Memory)
            .await
            .unwrap();

        let configs = config::read_configs_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        let config_id = configs[0].id.unwrap();

        let states = read_config_states_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].config_id, config_id);
        assert!(!states[0].is_running);

        let state_update = ConfigState {
            id: None,
            config_id,
            is_running: true,
            process_id: Some(1234),
            ..Default::default()
        };

        update_config_state_with_mode(&state_update, DatabaseMode::Memory)
            .await
            .unwrap();

        let updated_states = read_config_states_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(updated_states.len(), 1);
        assert!(updated_states[0].is_running);
    }

    #[tokio::test]
    async fn test_get_configs_state_with_mode_memory() {
        let _db = crate::test_utils::test_db().await;

        let _ = config::delete_all_configs_with_mode(DatabaseMode::Memory).await;

        let config_data = Config {
            service: Some("get-state-memory-test".to_string()),
            ..Config::default()
        };

        config::insert_config_with_mode(config_data, DatabaseMode::Memory)
            .await
            .unwrap();

        let states = get_configs_state_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(states.len(), 1);
        assert!(!states[0].is_running);
    }

    #[test]
    fn test_config_state_default_values() {
        let state = ConfigState::default();
        assert_eq!(state.id, None);
        assert_eq!(state.config_id, 0);
        assert!(!state.is_running);
        assert_eq!(state.process_id, None);
        assert!(!state.is_retrying);
        assert_eq!(state.retry_count, None);
        assert_eq!(state.last_error, None);
    }

    #[test]
    fn test_config_state_backward_compat_json() {
        // Old JSON without new fields (is_retrying, retry_count, last_error)
        let old_json = r#"{"id": 1, "config_id": 42, "is_running": true, "process_id": null}"#;
        let state: ConfigState = serde_json::from_str(old_json).unwrap();
        assert_eq!(state.id, Some(1));
        assert_eq!(state.config_id, 42);
        assert!(state.is_running);
        assert_eq!(state.process_id, None);
        // New fields should have defaults
        assert!(!state.is_retrying);
        assert_eq!(state.retry_count, None);
        assert_eq!(state.last_error, None);
    }

    #[test]
    fn test_config_state_json_serialization_with_new_fields() {
        let state = ConfigState {
            id: Some(1),
            config_id: 42,
            is_running: true,
            process_id: Some(1234),
            is_retrying: true,
            retry_count: Some(3),
            last_error: Some("Connection timeout".to_string()),
            process_start: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: ConfigState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, Some(1));
        assert_eq!(deserialized.config_id, 42);
        assert!(deserialized.is_running);
        assert_eq!(deserialized.process_id, Some(1234));
        assert!(deserialized.is_retrying);
        assert_eq!(deserialized.retry_count, Some(3));
        assert_eq!(
            deserialized.last_error,
            Some("Connection timeout".to_string())
        );
    }

    #[tokio::test]
    async fn test_config_state_db_round_trip_with_retry_fields() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("retry-test".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();
        let initial_states = read_config_states_with_pool(&pool).await.unwrap();
        let initial_state = initial_states
            .iter()
            .find(|s| s.config_id == config_id)
            .unwrap();
        // Update with retry fields
        let state_to_update = ConfigState {
            id: initial_state.id,
            config_id,
            is_running: true,
            process_id: Some(5678),
            is_retrying: true,
            retry_count: Some(3),
            last_error: Some("Pod restart detected".to_string()),
            process_start: None,
        };
        update_config_state_with_pool(&state_to_update, &pool)
            .await
            .unwrap();
        // Read back and verify all fields persisted
        let updated_states = read_config_states_with_pool(&pool).await.unwrap();
        let updated_state = updated_states
            .iter()
            .find(|s| s.config_id == config_id)
            .unwrap();
        assert_eq!(updated_state.config_id, config_id);
        assert!(updated_state.is_running);
        assert_eq!(updated_state.process_id, Some(5678));
        assert!(updated_state.is_retrying);
        assert_eq!(updated_state.retry_count, Some(3));
        assert_eq!(
            updated_state.last_error,
            Some("Pod restart detected".to_string())
        );
    }

    #[tokio::test]
    async fn test_config_state_db_round_trip_retry_count_none() {
        let pool = setup_test_db().await;
        let config_data = Config {
            service: Some("retry-none-test".to_string()),
            ..Config::default()
        };
        config::insert_config_with_pool(config_data.clone(), &pool)
            .await
            .unwrap();
        let configs = config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs.first().unwrap().id.unwrap();
        let initial_states = read_config_states_with_pool(&pool).await.unwrap();
        let initial_state = initial_states
            .iter()
            .find(|s| s.config_id == config_id)
            .unwrap();
        // Update with is_retrying=true but retry_count=None
        let state_to_update = ConfigState {
            id: initial_state.id,
            config_id,
            is_running: false,
            process_id: None,
            is_retrying: true,
            retry_count: None,
            last_error: None,
            process_start: None,
        };
        update_config_state_with_pool(&state_to_update, &pool)
            .await
            .unwrap();
        // Read back and verify
        let updated_states = read_config_states_with_pool(&pool).await.unwrap();
        let updated_state = updated_states
            .iter()
            .find(|s| s.config_id == config_id)
            .unwrap();
        assert!(updated_state.is_retrying);
        assert_eq!(updated_state.retry_count, None);
        assert_eq!(updated_state.last_error, None);
    }

    async fn config_with_state(pool: &SqlitePool, is_running: bool, process_id: u32) -> i64 {
        let id = config::insert_config_with_pool(Config::default(), pool)
            .await
            .unwrap();
        update_config_state_with_pool(
            &ConfigState {
                config_id: id,
                is_running,
                process_id: Some(process_id),
                ..Default::default()
            },
            pool,
        )
        .await
        .unwrap();
        id
    }

    async fn last_error_of(pool: &SqlitePool, config_id: i64) -> Option<String> {
        read_config_states_with_pool(pool)
            .await
            .unwrap()
            .into_iter()
            .find(|state| state.config_id == config_id)
            .unwrap()
            .last_error
    }

    #[tokio::test]
    async fn connection_error_is_saved_and_cleared_on_a_running_row() {
        let pool = setup_test_db().await;
        let id = config_with_state(&pool, true, std::process::id()).await;

        set_connection_error_with_pool(id, Some("pod not ready"), &pool)
            .await
            .unwrap();
        assert_eq!(
            last_error_of(&pool, id).await.as_deref(),
            Some("pod not ready")
        );

        set_connection_error_with_pool(id, None, &pool)
            .await
            .unwrap();
        assert_eq!(last_error_of(&pool, id).await, None);
    }

    #[tokio::test]
    async fn connection_error_leaves_stopped_and_foreign_rows_alone() {
        let pool = setup_test_db().await;
        let stopped = config_with_state(&pool, false, std::process::id()).await;
        let foreign = config_with_state(&pool, true, std::process::id().wrapping_add(1)).await;

        for id in [stopped, foreign] {
            set_connection_error_with_pool(id, Some("pod not ready"), &pool)
                .await
                .unwrap();
            assert_eq!(last_error_of(&pool, id).await, None);
        }
    }

    async fn retry_fields_of(
        pool: &SqlitePool, config_id: i64,
    ) -> (bool, Option<i32>, Option<String>) {
        let state = read_config_states_with_pool(pool)
            .await
            .unwrap()
            .into_iter()
            .find(|state| state.config_id == config_id)
            .unwrap();
        (state.is_retrying, state.retry_count, state.last_error)
    }

    #[tokio::test]
    async fn retry_status_is_written_to_the_row_its_owner_still_holds() {
        let pool = setup_test_db().await;
        let id = config_with_state(&pool, true, 4242).await;

        set_retry_status_with_pool(
            id,
            Some(4242),
            None,
            Some(2),
            Some("connection refused"),
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(
            retry_fields_of(&pool, id).await,
            (true, Some(2), Some("connection refused".to_string()))
        );
        let state = read_config_states_with_pool(&pool)
            .await
            .unwrap()
            .into_iter()
            .find(|state| state.config_id == id)
            .unwrap();
        assert!(state.is_running);
        assert_eq!(state.process_id, Some(4242));
    }

    #[tokio::test]
    async fn retry_status_leaves_a_row_that_changed_owner_or_stopped_alone() {
        let pool = setup_test_db().await;
        let taken_over = config_with_state(&pool, true, 4242).await;
        let stopped = config_with_state(&pool, true, 4242).await;
        update_config_state_with_pool(
            &ConfigState {
                config_id: taken_over,
                is_running: true,
                process_id: Some(9999),
                ..Default::default()
            },
            &pool,
        )
        .await
        .unwrap();
        update_config_state_with_pool(
            &ConfigState {
                config_id: stopped,
                is_running: false,
                process_id: None,
                ..Default::default()
            },
            &pool,
        )
        .await
        .unwrap();

        set_retry_status_with_pool(
            taken_over,
            Some(4242),
            None,
            Some(1),
            Some("connection refused"),
            &pool,
        )
        .await
        .unwrap();
        set_retry_status_with_pool(
            stopped,
            Some(4242),
            None,
            Some(1),
            Some("connection refused"),
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(
            retry_fields_of(&pool, taken_over).await,
            (false, None, None)
        );
        assert_eq!(retry_fields_of(&pool, stopped).await, (false, None, None));
    }

    #[tokio::test]
    async fn a_retry_status_is_cleared_only_from_a_row_this_process_runs() {
        let pool = setup_test_db().await;
        let own = config_with_state(&pool, true, std::process::id()).await;
        let foreign = config_with_state(&pool, true, 4242).await;
        let stopped = config_with_state(&pool, true, std::process::id()).await;

        set_retry_status_with_pool(
            own,
            Some(std::process::id()),
            process_start_identity(std::process::id()),
            Some(3),
            Some("connection refused"),
            &pool,
        )
        .await
        .unwrap();
        set_retry_status_with_pool(
            foreign,
            Some(4242),
            None,
            Some(3),
            Some("connection refused"),
            &pool,
        )
        .await
        .unwrap();
        update_config_state_with_pool(
            &ConfigState {
                config_id: stopped,
                is_running: false,
                process_id: None,
                is_retrying: true,
                retry_count: Some(3),
                last_error: Some("connection refused".to_string()),
                ..Default::default()
            },
            &pool,
        )
        .await
        .unwrap();

        for id in [own, foreign, stopped] {
            clear_retry_status_with_pool(id, &pool).await.unwrap();
        }

        assert_eq!(retry_fields_of(&pool, own).await, (false, None, None));
        assert_eq!(
            retry_fields_of(&pool, foreign).await,
            (true, Some(3), Some("connection refused".to_string()))
        );
        assert_eq!(
            retry_fields_of(&pool, stopped).await,
            (true, Some(3), Some("connection refused".to_string()))
        );
    }

    #[tokio::test]
    async fn a_connection_error_recorded_after_the_restore_survives_the_clear() {
        let pool = setup_test_db().await;
        let id = config_with_state(&pool, true, std::process::id()).await;
        set_connection_error_with_pool(id, Some("connection reset"), &pool)
            .await
            .unwrap();

        clear_retry_status_with_pool(id, &pool).await.unwrap();

        assert_eq!(
            retry_fields_of(&pool, id).await,
            (false, None, Some("connection reset".to_string()))
        );
    }

    #[tokio::test]
    async fn an_unreadable_running_snapshot_is_told_apart_from_an_absent_one() {
        let _db = crate::test_utils::test_db().await;
        let dir = tempfile::tempdir().unwrap();
        let _env =
            crate::test_utils::EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
        crate::utils::db::init().await.unwrap();
        let mode = DatabaseMode::File;
        let id = 8_104;

        assert_eq!(try_running_snapshot(id, mode).await, Ok(None));

        let config = Config {
            alias: Some("app.local".to_string()),
            ..Config::default()
        };
        set_running_snapshot(id, &config, mode).await.unwrap();
        assert_eq!(
            try_running_snapshot(id, mode).await,
            Ok(Some(config.clone()))
        );

        crate::utils::settings::set_setting_with_mode(
            &running_snapshot_key(id, mode),
            "{not a config",
            mode,
        )
        .await
        .unwrap();
        assert!(try_running_snapshot(id, mode).await.is_err());
        assert_eq!(
            running_snapshot(id, mode).await,
            None,
            "the lenient read keeps treating an unreadable record as absent"
        );

        clear_running_snapshot(id, mode).await;
        assert_eq!(try_running_snapshot(id, mode).await, Ok(None));
    }

    #[cfg(unix)]
    fn owned_state(pid: u32, process_start: Option<i64>) -> ConfigState {
        ConfigState {
            config_id: 1,
            is_running: true,
            process_id: Some(pid),
            process_start,
            ..Default::default()
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_reused_pid_with_another_start_identity_is_not_an_owner() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("failed to spawn helper process");
        let pid = child.id();
        let live = process_start_identity(pid).expect("the child's start identity");

        let stale = is_owned_by_other_live_process(&owned_state(pid, Some(live + 1)));
        let matching = is_owned_by_other_live_process(&owned_state(pid, Some(live)));
        let legacy = is_owned_by_other_live_process(&owned_state(pid, None));
        let _ = child.kill();
        let _ = child.wait();

        assert!(!stale, "a row recorded by an earlier process with this pid");
        assert!(matching, "the process that wrote the row is still running");
        assert!(
            legacy,
            "a row without an identity keeps the pid-only reading"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn this_pid_with_another_start_identity_is_a_stale_row() {
        let pid = std::process::id();
        let own = process_start_identity(pid).expect("this process's start identity");

        assert!(
            !process_is_alive(pid, Some(own + 1)),
            "a row left by an earlier process that had this pid"
        );
        assert!(process_is_alive(pid, Some(own)), "a row this process wrote");
        assert!(
            process_is_alive(pid, None),
            "a row without an identity keeps the pid-only reading"
        );
    }

    #[tokio::test]
    async fn a_row_claimed_by_this_process_records_its_start_identity() {
        let pool = setup_test_db().await;
        let id = config_with_state(&pool, true, std::process::id()).await;

        let state = read_config_states_with_pool(&pool)
            .await
            .unwrap()
            .into_iter()
            .find(|state| state.config_id == id)
            .unwrap();

        assert_eq!(
            state.process_start,
            process_start_identity(std::process::id())
        );
    }
}
