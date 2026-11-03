//! OIDC exchange, trust policies and the issuer fetch guard, driven against the real
//! `oxabl-cache-server` binary and a fake identity provider inside a network namespace.

mod exchange;
mod logs;
mod policies;
mod rebinding;
mod rejections;
mod ssrf;
mod support;
