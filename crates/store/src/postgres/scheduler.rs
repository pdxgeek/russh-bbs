//! The scheduler's persisted last runs over Postgres (M3-C, D79).

use russh_bbs_core::access::Slug;
use russh_bbs_core::scheduler::{LastRun, RunOutcome, SchedulerStore};
use russh_bbs_core::store::{StoreError, UnixSeconds};

use super::PostgresStore;
use crate::error::backend;

impl SchedulerStore for PostgresStore {
    async fn last_run(&self, job: &Slug) -> Result<Option<LastRun>, StoreError> {
        let name = job.as_str();
        let row = sqlx::query!(
            "SELECT last_run_at, last_outcome FROM scheduler_job WHERE name = $1",
            name,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map(|row| {
            Ok(LastRun {
                at: UnixSeconds::from_raw(row.last_run_at),
                outcome: RunOutcome::parse(&row.last_outcome)
                    .ok_or_else(|| StoreError::Corrupt("stored job outcome is unknown".into()))?,
            })
        })
        .transpose()
    }

    async fn record_run(&self, job: &Slug, run: LastRun) -> Result<(), StoreError> {
        let name = job.as_str();
        let at = run.at.as_raw();
        let outcome = run.outcome.name();
        sqlx::query!(
            "INSERT INTO scheduler_job (name, last_run_at, last_outcome) VALUES ($1, $2, $3)
             ON CONFLICT (name) DO UPDATE
             SET last_run_at = excluded.last_run_at, last_outcome = excluded.last_outcome",
            name,
            at,
            outcome,
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(())
    }
}
