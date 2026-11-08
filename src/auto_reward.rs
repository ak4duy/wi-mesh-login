use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use time::{OffsetDateTime, UtcOffset};

use crate::reward;

const RETRY_AFTER: Duration = Duration::from_secs(30 * 60);
const PORTAL_OFFSET_HOURS: i8 = 7;

pub(crate) struct DailyClaim {
    retry_at: Option<Instant>,
}

impl DailyClaim {
    pub(crate) fn new() -> Self {
        Self { retry_at: None }
    }

    pub(crate) fn tick(&mut self) {
        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return;
        }
        match claim_once_today() {
            Ok(()) => self.retry_at = None,
            Err(error) => {
                eprintln!(
                    "Daily reward failed: {error:#}. Retrying in {}m.",
                    RETRY_AFTER.as_secs() / 60
                );
                self.retry_at = Some(Instant::now() + RETRY_AFTER);
            }
        }
    }
}

fn claim_file() -> Result<PathBuf> {
    Ok(reward::default_state_dir()?.join("last-claim"))
}

fn today() -> String {
    let offset = UtcOffset::from_hms(PORTAL_OFFSET_HOURS, 0, 0).unwrap_or(UtcOffset::UTC);
    OffsetDateTime::now_utc()
        .to_offset(offset)
        .date()
        .to_string()
}

fn claim_once_today() -> Result<()> {
    let path = claim_file()?;
    let today = today();
    if fs::read_to_string(&path).is_ok_and(|saved| saved.trim() == today) {
        return Ok(());
    }
    let mut session = reward::authenticate_saved(None)?;
    reward::claim_rewards(&mut session)?;
    reward::write_private(&path, &today)
}
