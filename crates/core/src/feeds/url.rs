//! A feed's address, validated before anything stores or fetches it.
//!
//! This is the text half of the SSRF guard (threat-model.md, "SSRF guard"): only `http` and
//! `https`, no credentials in the URL, a host, bounded length, no control characters. The
//! other half (resolving the name and refusing private addresses at connect time) is the
//! fetcher's, because only it can see the addresses.

use url::{Host, Url};

/// Longest URL accepted, in bytes (also the database's `CHECK`).
pub const MAX_URL_BYTES: usize = 2048;

/// Why an address was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    /// Empty, over [`MAX_URL_BYTES`], or holding a control character or whitespace.
    #[error("that is not a usable address")]
    Malformed,
    /// Not an `http` or `https` URL with a host.
    #[error("only http and https addresses are supported")]
    Scheme,
    /// Plain `http` without the sysop's per-feed flag (D89).
    #[error("plain http needs the allow-http flag")]
    NotHttps,
    /// The URL carries a user name or password.
    #[error("the address may not carry a user name or password")]
    Credentials,
}

/// A validated feed address in its normalised form (lower-case host, IDNA host in punycode,
/// numeric IPv4 forms such as `0x7f.1` rewritten to dotted decimal, no fragment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedUrl(Url);

impl FeedUrl {
    /// Validate `raw`. Plain `http` is accepted only when `allow_http` is set.
    ///
    /// # Errors
    /// [`UrlError`] for anything the rules above refuse.
    pub fn parse(raw: &str, allow_http: bool) -> Result<Self, UrlError> {
        let raw = raw.trim();
        // The URL parser silently deletes tabs and newlines, so a control character must be
        // refused before it gets there.
        if raw.is_empty()
            || raw.len() > MAX_URL_BYTES
            || raw.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(UrlError::Malformed);
        }
        let mut url = Url::parse(raw).map_err(|_| UrlError::Malformed)?;
        Self::check(&mut url, allow_http)?;
        Ok(Self(url))
    }

    /// Check a URL that came from a redirect's `Location`, resolved against the current one.
    ///
    /// # Errors
    /// The same as [`FeedUrl::parse`].
    pub fn join(&self, location: &str, allow_http: bool) -> Result<Self, UrlError> {
        if location.len() > MAX_URL_BYTES
            || location
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(UrlError::Malformed);
        }
        let mut url = self.0.join(location).map_err(|_| UrlError::Malformed)?;
        Self::check(&mut url, allow_http)?;
        Ok(Self(url))
    }

    fn check(url: &mut Url, allow_http: bool) -> Result<(), UrlError> {
        match url.scheme() {
            "https" => {}
            "http" if allow_http => {}
            "http" => return Err(UrlError::NotHttps),
            _ => return Err(UrlError::Scheme),
        }
        if url.host().is_none() {
            return Err(UrlError::Scheme);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(UrlError::Credentials);
        }
        url.set_fragment(None);
        if url.as_str().len() > MAX_URL_BYTES {
            return Err(UrlError::Malformed);
        }
        Ok(())
    }

    /// The normalised address.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The parsed URL, for the fetcher.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.0
    }

    /// Whether the scheme is `https`.
    #[must_use]
    pub fn is_https(&self) -> bool {
        self.0.scheme() == "https"
    }

    /// The host, as the URL parser understood it.
    #[must_use]
    pub fn host(&self) -> Option<Host<&str>> {
        self.0.host()
    }
}
