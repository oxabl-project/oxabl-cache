use url::Url;

use crate::config::{ConfigError, check_server};

/// An endpoint must be `https`, or `http` on a loopback address, and carry no credentials,
/// query or fragment.
pub(super) fn check_endpoint(url: &Url) -> Result<(), ConfigError> {
    check_server(url)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::Invalid(
            "an endpoint carries credentials, a query or a fragment",
        ));
    }
    Ok(())
}

/// A string safe to show a person: bounded and free of control characters.
pub(super) fn displayable(text: &str) -> bool {
    !text.is_empty() && text.len() <= 512 && !text.chars().any(char::is_control)
}
