//! argon2id hashing in the PHC string format (D79).
//!
//! A stored hash is the whole PHC string, `$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`, so its
//! parameters travel with it: raising [`MEMORY_KIB`] or [`ITERATIONS`] later changes what new
//! hashes use, old ones still verify with the parameters they carry, and [`needs_rehash`] tells
//! a login to upgrade them.
//!
//! Verification accepts only `argon2id` and refuses a string whose memory cost is absurd
//! ([`MAX_ACCEPTED_MEMORY_KIB`]), so an edited row cannot make a login allocate gigabytes.

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, try_generate_salt};
use argon2::{Algorithm, Argon2, Params, Version};

/// Memory cost in KiB (19 MiB), the OWASP minimum for argon2id with two iterations.
pub const MEMORY_KIB: u32 = 19 * 1024;
/// Iterations.
pub const ITERATIONS: u32 = 2;
/// Parallel lanes. One, so a hash holds one core.
pub const LANES: u32 = 1;
/// The largest memory cost a stored hash may carry and still be verified.
pub const MAX_ACCEPTED_MEMORY_KIB: u32 = 256 * 1024;

/// A hash made with the current parameters of a password nobody has. A login for a name that
/// does not exist, has no password or is not active verifies against it, so those logins cost
/// what a wrong password costs (D79). A test checks it parses and is current.
pub(crate) const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$ZHVtbXktc2FsdC0xMjM0NQ$xwvTi6nRH+Z7e2rjusTWEqh+rgUtQVcjVqf3FoRaEZk";

/// Hashing failed (the system random source). Nothing was stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("could not hash the password")]
pub struct HashError;

fn current() -> Result<Argon2<'static>, HashError> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, LANES, None).map_err(|_| HashError)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Hash `password` with the current parameters and a fresh random salt.
///
/// # Errors
/// [`HashError`] if the random source or the hasher fails.
pub fn hash(password: &str) -> Result<String, HashError> {
    let salt = try_generate_salt().map_err(|_| HashError)?;
    let made = current()?
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|_| HashError)?;
    Ok(made.to_string())
}

/// Parse `stored` if it is an argon2id hash this server would verify.
fn accepted(stored: &str) -> Option<(PasswordHash, Params)> {
    let parsed = PasswordHash::new(stored).ok()?;
    if parsed.algorithm.as_str() != Algorithm::Argon2id.ident().as_str() {
        return None;
    }
    let params = Params::try_from(&parsed).ok()?;
    (params.m_cost() <= MAX_ACCEPTED_MEMORY_KIB).then_some((parsed, params))
}

/// Whether `password` matches `stored`. Anything unparseable or not argon2id is a mismatch.
#[must_use]
pub fn verify(stored: &str, password: &str) -> bool {
    let Some((parsed, _)) = accepted(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Whether `stored` was made with weaker parameters than the current ones (or is not a hash
/// this server verifies), so a successful login should store a new hash.
#[must_use]
pub fn needs_rehash(stored: &str) -> bool {
    let Some((parsed, params)) = accepted(stored) else {
        return false;
    };
    params.m_cost() < MEMORY_KIB
        || params.t_cost() < ITERATIONS
        || params.p_cost() < LANES
        || parsed.version != Some(Version::V0x13.into())
}
