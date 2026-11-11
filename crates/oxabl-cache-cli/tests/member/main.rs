//! The `oxabl-cache` member command line, driven as a binary against the real server, with the
//! operator's `oxabl-cache-admin` for setup and the client library as oxabl uses it.

mod support;

mod dictionary;
mod entries;
mod environment;
mod error_log;
mod flow;
mod invite;
mod keyring;
mod login;
mod onboard;
mod owners;
mod roster_guards;
mod secrets;
mod verify_sample;
