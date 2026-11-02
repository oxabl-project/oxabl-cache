//! The real `oxabl-cache-server` binary driven over raw HTTP on a fresh data
//! directory. Records and segments are random bytes of valid outer shape: the
//! server never opens them.

mod commit_retry;
mod config_rules;
mod first_write_wins;
mod gc;
mod lifecycle;
mod limits;
mod logs;
mod object_urls;
mod roster_rules;
mod round_trip;
mod scopes;
mod support;
