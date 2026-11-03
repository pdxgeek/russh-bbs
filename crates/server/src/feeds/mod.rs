//! The server half of feeds (M3-G, D89): the SSRF-guarded fetcher, the scheduler job that reads
//! the feeds that are due, and the RSS-out listener. The rules (what a feed is, how a document
//! is read, what an item becomes, who may read a board as RSS) are in `russh_bbs_core::feeds`;
//! nothing here decides policy.

mod fetch;
mod guard;
mod job;
mod rss_out;

pub use fetch::{FetchLimits, GuardedFetcher, USER_AGENT_VALUE, proxy_in_environment};
pub use guard::{Guard, Resolve, SystemResolver, is_public_ip};
pub use job::{CHECK_EVERY, JOB_NAME, MAX_FEEDS_PER_RUN, job};
pub use rss_out::{RssOut, RssOutConfig};

#[cfg(test)]
mod tests;
