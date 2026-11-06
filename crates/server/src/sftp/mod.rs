//! The SFTP subsystem (M4-F2, D141): file areas over `sftp`, on the SSH transport only.
//!
//! The subsystem serves a **virtual file system addressed by id** (`russh_bbs_core::files::path`
//! is the one parser): `/<area>/<file>`, `/<area>/pending/<file>` and `/downloads/<file>`, with
//! no real path behind any of it, no symlinks and no directories a client can make. Every
//! operation loads the member's access fresh and goes through `core::files`; this module only
//! speaks the protocol (russh-sftp) and moves bytes through the blob store.
//!
//! * **Download:** a read handle streams the blob (verifying its hash at the end); a corrupt
//!   object fails the read instead of ending quietly.
//! * **Upload:** an open for writing is planned by core (permission, name, limits), the writes
//!   are metered as they arrive and streamed into `BlobStore::put` under the `quarantine`
//!   namespace; closing the handle commits, records the file, scans it and applies the area's
//!   rule. Writes must arrive in order (the OpenSSH client's do).
//! * **Not supported, and said so:** mkdir, rmdir, rename, symlinks, links, `setstat`, append,
//!   overwrite. A file cannot be changed once uploaded; removal is for a member's own
//!   quarantined uploads (and for approvers).
//!
//! Telnet has no SFTP: the Files screens say downloads need ssh.

mod scan;
mod session;
mod upload;

pub use scan::{ClamdScanner, ClamdTarget, Scanner};
pub(crate) use session::serve;

use russh_bbs_store::blobs::Blobs;

/// What the file features need beyond the database: the blob store, the scanner, and whether
/// SFTP is on. Built once at start-up and shared by every session.
#[derive(Debug, Clone)]
pub struct FileServices {
    /// Where the bytes live.
    pub blobs: Blobs,
    /// What checks uploads.
    pub scanner: Scanner,
    /// Whether the `sftp` subsystem is served.
    pub sftp: bool,
}

/// How the Files screens may fetch in a session: over SFTP on ssh when the board runs the
/// subsystem, browse-only over telnet or with SFTP off, and nothing on a board built without a
/// file store.
#[must_use]
pub fn files_mode(ui: &crate::ui::Ui, telnet: bool) -> russh_bbs_core::files::FilesMode {
    use russh_bbs_core::files::FilesMode;
    match ui.files() {
        None => FilesMode::Unavailable,
        Some(_) if telnet => FilesMode::Telnet,
        Some(services) if services.sftp => FilesMode::Sftp,
        Some(_) => FilesMode::NoSftp,
    }
}
