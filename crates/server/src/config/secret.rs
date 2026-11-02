//! A datasource URL that may carry a password. Its `Debug` and `Display` redact it, so logging
//! the whole config can never leak one. Only [`SecretUrl::expose`] returns the real text, and
//! only the backend that opens the connection should call it.

use std::fmt;

use serde::{Deserialize, Deserializer};

/// Mask shown in place of a secret.
const MASK: &str = "***";

/// A URL whose password (and password-like query values) must not be printed.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretUrl(String);

impl SecretUrl {
    /// Wrap a URL.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    /// The real URL, for the code that connects. Never log this.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The URL with its password masked, safe to print.
    #[must_use]
    pub fn redacted(&self) -> String {
        redact(&self.0)
    }
}

impl fmt::Debug for SecretUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretUrl({})", self.redacted())
    }
}

impl fmt::Display for SecretUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

impl<'de> Deserialize<'de> for SecretUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self)
    }
}

/// Mask the password in `url`. Conservative on purpose: userinfo ends at the **last** `@` in the
/// whole text, so an unescaped `/`, `?`, `#` or `@` in a password cannot leave a fragment
/// visible (at worst a query value containing `@` is over-masked). Any query parameter whose
/// name looks secret is masked. Text that is not a URL is not echoed.
fn redact(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "<not a URL>".to_owned();
    };
    let mut out = format!("{scheme}://");
    let tail = match rest.rfind('@') {
        Some(at) => {
            let userinfo = &rest[..at];
            let user = userinfo.split(':').next().unwrap_or_default();
            out.push_str(user);
            if userinfo.contains(':') {
                out.push(':');
                out.push_str(MASK);
            }
            out.push('@');
            &rest[at + 1..]
        }
        None => rest,
    };
    match tail.find(['?', '#']) {
        Some(i) => {
            out.push_str(&tail[..i]);
            out.push_str(&redact_query(&tail[i..]));
        }
        None => out.push_str(tail),
    }
    out
}

/// Mask the value of every `name=value` pair whose name looks secret. `query` starts with the
/// `?` or `#` that introduced it.
fn redact_query(query: &str) -> String {
    let (lead, body) = query.split_at(1);
    let pairs: Vec<String> = body
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) if looks_secret(name) => format!("{name}={MASK}"),
            _ => pair.to_owned(),
        })
        .collect();
    format!("{lead}{}", pairs.join("&"))
}

fn looks_secret(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["pass", "secret", "token", "key"]
        .iter()
        .any(|word| lower.contains(word))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::SecretUrl;

    #[test]
    fn password_is_masked_everywhere_it_is_printed() {
        let url = SecretUrl::new("postgres://bbs:hunter2@db.example:5432/bbs?sslmode=require");
        for text in [format!("{url}"), format!("{url:?}"), url.redacted()] {
            assert!(!text.contains("hunter2"), "{text}");
            assert!(text.contains("bbs:***@db.example"), "{text}");
            assert!(text.contains("sslmode=require"), "{text}");
        }
        assert!(url.expose().contains("hunter2"));
    }

    #[test]
    fn awkward_passwords_do_not_leak_a_fragment() {
        for pw in ["a/b", "a@b", "p?q", "x:y", "#frag"] {
            let url = SecretUrl::new(format!("postgres://u:{pw}@h/db"));
            let shown = url.redacted();
            assert!(!shown.contains(pw), "{pw} -> {shown}");
        }
    }

    #[test]
    fn password_in_the_query_is_masked() {
        let url = SecretUrl::new("postgres://u@h/db?password=s3cret&sslmode=require");
        let shown = url.redacted();
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains("password=***&sslmode=require"), "{shown}");
    }

    #[test]
    fn a_non_url_is_not_echoed() {
        assert_eq!(SecretUrl::new("hunter2").redacted(), "<not a URL>");
    }
}
