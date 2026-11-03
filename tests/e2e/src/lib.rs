//! Black-box test environment: supervised child processes, S3 servers, Postgres and a fault proxy.

pub mod cache_server;
pub mod cli;
pub mod fake_dns;
pub mod faketime;
pub mod fault_proxy;
pub mod garage;
pub mod idp;
pub mod keeper;
pub mod netns;
pub mod pdeathsig;
pub mod ports;
pub mod postgres;
pub mod random;
pub mod s3;
pub mod supervise;
pub mod versitygw;
pub mod wait;
pub mod workdir;
