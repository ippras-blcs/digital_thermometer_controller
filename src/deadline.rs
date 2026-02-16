use anyhow::Result;
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use log::debug;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{spawn, time::sleep};

const SLEEP: Duration = Duration::from_secs(1);
const TIMESTAMP: Duration = Duration::from_secs(1798750800); // 2027-01-01

pub(super) fn start() {
    spawn(run());
}

pub(super) async fn run() -> Result<()> {
    let sntp = EspSntp::new_default()?;
    while SyncStatus::Completed != sntp.get_sync_status() {
        debug!("SNTP is not completed, wait {}", SLEEP.as_secs());
        sleep(SLEEP).await;
    }
    if SystemTime::now().duration_since(UNIX_EPOCH)? > TIMESTAMP {
        panic!("TIMESTAMP: {TIMESTAMP:?}");
    }
    Ok(())
}
