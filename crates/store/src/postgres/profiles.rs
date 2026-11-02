//! Profiles and the member list over Postgres (M2-F).
//!
//! Only `active` accounts are ever returned. Handles are ASCII (letters, digits, `_`, `-`) and
//! unique ignoring case, so `lower(username)` is both the sort key and the paging cursor, and
//! the `"C"` collation is byte order, the same order SQLite's BINARY gives.

use russh_bbs_core::access::AccessStore;
use russh_bbs_core::audit::AuditEvent;
use russh_bbs_core::profiles::{MemberQuery, MemberRecord, MemberSummary, Profile, ProfileStore};
use russh_bbs_core::store::{AccountId, StoreError};
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::error::{backend, kind};
use crate::profile_rows::{RecordRow, SummaryRow};

impl ProfileStore for PostgresStore {
    async fn list_members(
        &self,
        query: &MemberQuery,
        limit: usize,
    ) -> Result<Vec<MemberSummary>, StoreError> {
        let after = query.after.as_ref().map_or("", |cursor| cursor.as_str());
        let prefix = query.prefix.as_str();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = sqlx::query_as!(
            SummaryRow,
            r#"SELECT a.id AS "id!", a.username AS "username!",
                      COALESCE(p.display_name, '') AS "display_name!"
               FROM accounts a LEFT JOIN profiles p ON p.account_id = a.id
               WHERE a.state = 'active'
                 AND lower(a.username) COLLATE "C" > $1
                 AND substr(lower(a.username), 1, length($2)) = $2
               ORDER BY lower(a.username) COLLATE "C"
               LIMIT $3"#,
            after,
            prefix,
            limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        rows.into_iter().map(SummaryRow::into_summary).collect()
    }

    async fn member(&self, id: AccountId) -> Result<Option<MemberRecord>, StoreError> {
        let id = id.as_raw();
        let row = sqlx::query_as!(
            RecordRow,
            r#"SELECT a.id AS "id!", a.username AS "username!",
                      COALESCE(p.display_name, '') AS "display_name!",
                      COALESCE(p.pronouns, '') AS "pronouns!",
                      COALESCE(p.location, '') AS "location!",
                      COALESCE(p.about, '') AS "about!",
                      COALESCE(p.website, '') AS "website!"
               FROM accounts a LEFT JOIN profiles p ON p.account_id = a.id
               WHERE a.id = $1 AND a.state = 'active'"#,
            id,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map(RecordRow::into_record).transpose()
    }

    async fn standing(&self, id: AccountId) -> Result<Option<u8>, StoreError> {
        // The same facts the editing services use, read the same way (D69), so the rank rule
        // for profiles cannot drift from the one for roles.
        Ok(self.snapshot(Some(id)).await?.top_rank())
    }

    async fn save_profile(
        &self,
        id: AccountId,
        profile: &Profile,
        audit: Option<&AuditEvent>,
    ) -> Result<(), StoreError> {
        use russh_bbs_core::profiles::Field;
        let id = id.as_raw();
        let display_name = profile.get(Field::DisplayName);
        let pronouns = profile.get(Field::Pronouns);
        let location = profile.get(Field::Location);
        let about = profile.get(Field::About);
        let website = profile.get(Field::Website);
        let mut tx = self.pool().begin().await.map_err(backend)?;
        sqlx::query!(
            "INSERT INTO profiles (account_id, display_name, pronouns, location, about, website)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (account_id) DO UPDATE SET
                 display_name = excluded.display_name,
                 pronouns     = excluded.pronouns,
                 location     = excluded.location,
                 about        = excluded.about,
                 website      = excluded.website,
                 updated_at   = extract(epoch FROM now())::bigint",
            id,
            display_name,
            pronouns,
            location,
            about,
            website,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?;
        if let Some(event) = audit {
            self.audit_in(&mut tx, event).await?;
        }
        tx.commit().await.map_err(backend)
    }
}
