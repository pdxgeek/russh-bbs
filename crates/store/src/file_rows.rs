//! Backend-independent mapping for the file tables: what a stored row means, written once
//! (D24). Each backend runs its own SQL and hands the values here.

use russh_bbs_core::files::{AreaId, AreaRow, FileId, FileRow, FileState, ScanState};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

fn count(value: i64, what: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt(format!("{what} is negative")))
}

pub(crate) fn area(
    id: i64,
    name: String,
    description: String,
    restricted: bool,
    auto_approve: bool,
    files: i64,
) -> Result<AreaRow, StoreError> {
    Ok(AreaRow {
        id: AreaId::from_raw(id),
        name,
        description,
        restricted,
        auto_approve,
        files: count(files, "an area's file count")?,
    })
}

/// A file row as the backends select it (the tags come from a second query).
pub(crate) struct FileRecord {
    pub(crate) id: i64,
    pub(crate) area_id: i64,
    pub(crate) area_name: String,
    pub(crate) uploader_id: Option<i64>,
    pub(crate) uploader: Option<String>,
    pub(crate) name: String,
    pub(crate) original_name: String,
    pub(crate) description: String,
    pub(crate) size: i64,
    pub(crate) sha256: String,
    pub(crate) blob_key: String,
    pub(crate) state: String,
    pub(crate) scan: String,
    pub(crate) scan_note: String,
    pub(crate) created_at: i64,
    pub(crate) decided_at: Option<i64>,
    pub(crate) downloads: i64,
}

impl FileRecord {
    /// The row, with its tags.
    pub(crate) fn into_row(self, tags: Vec<String>) -> Result<FileRow, StoreError> {
        let state = self
            .state
            .parse::<FileState>()
            .map_err(|_| StoreError::Corrupt("a file has an unknown state".into()))?;
        let scan = self
            .scan
            .parse::<ScanState>()
            .map_err(|_| StoreError::Corrupt("a file has an unknown scan state".into()))?;
        Ok(FileRow {
            id: FileId::from_raw(self.id),
            area: AreaId::from_raw(self.area_id),
            area_name: self.area_name,
            uploader_id: self.uploader_id.map(AccountId::from_raw),
            uploader: self.uploader,
            name: self.name,
            original_name: self.original_name,
            description: self.description,
            size: count(self.size, "a file's size")?,
            sha256: self.sha256,
            blob_key: self.blob_key,
            state,
            scan,
            scan_note: self.scan_note,
            created_at: UnixSeconds::from_raw(self.created_at),
            decided_at: self.decided_at.map(UnixSeconds::from_raw),
            downloads: count(self.downloads, "a file's download count")?,
            tags,
        })
    }
}

/// The lower-cased name that makes two areas differ by more than case.
pub(crate) fn name_key(name: &str) -> String {
    name.to_lowercase()
}

/// `[1,2,3]` for a JSON array of ids (SQLite binds it to `json_each`).
pub(crate) fn json_area_ids(areas: &[AreaId]) -> String {
    let list: Vec<String> = areas.iter().map(|a| a.as_raw().to_string()).collect();
    format!("[{}]", list.join(","))
}

/// The area ids as numbers (Postgres binds them as an array).
pub(crate) fn raw_area_ids(areas: &[AreaId]) -> Vec<i64> {
    areas.iter().map(|a| a.as_raw()).collect()
}

/// `[1,2,3]` for a JSON array of file ids.
pub(crate) fn json_file_ids(ids: &[i64]) -> String {
    let list: Vec<String> = ids.iter().map(ToString::to_string).collect();
    format!("[{}]", list.join(","))
}

/// The search terms as a JSON array of strings (SQLite). Terms are runs of letters and digits
/// (core's `SearchQuery`); anything else is dropped rather than trusted, so no quote or
/// backslash can reach the JSON.
pub(crate) fn json_terms(terms: &[String]) -> String {
    let list: Vec<String> = terms
        .iter()
        .filter(|t| !t.is_empty() && t.chars().all(char::is_alphanumeric))
        .map(|t| format!("\"{t}\""))
        .collect();
    format!("[{}]", list.join(","))
}

/// The search terms as a list (Postgres), under the same filter.
pub(crate) fn plain_term_list(terms: &[String]) -> Vec<String> {
    terms
        .iter()
        .filter(|t| !t.is_empty() && t.chars().all(char::is_alphanumeric))
        .cloned()
        .collect()
}

/// A number the database can compare with.
pub(crate) fn int(value: u32) -> i64 {
    i64::from(value)
}

/// A size as the database stores it.
pub(crate) fn size(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
