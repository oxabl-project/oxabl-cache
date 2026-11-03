//! The `DeleteObjects` answer, read without requiring an XML namespace: some
//! S3-compatible servers omit it.

use oxabl_cache_core::{Error, Result};

/// What a `DeleteObjects` answer accounts for.
pub(crate) struct DeleteAnswer {
    pub(crate) deleted: usize,
    pub(crate) error_codes: Vec<String>,
}

/// The inner text of every `<tag>…</tag>` element, attributes allowed on the opening tag.
fn elements<'a>(text: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        let Some(boundary) = after.chars().next() else {
            break;
        };
        if !matches!(boundary, '>' | ' ' | '\n' | '\t' | '/') {
            rest = after;
            continue;
        }
        let Some(start) = after.find('>') else { break };
        if after[..start].ends_with('/') {
            found.push("");
            rest = &after[start + 1..];
            continue;
        }
        let body = &after[start + 1..];
        let Some(end) = body.find(&close) else { break };
        found.push(&body[..end]);
        rest = &body[end + close.len()..];
    }
    found
}

/// Parses an answer; a body that is not a `DeleteResult` is an error.
pub(crate) fn parse(text: &str) -> Result<DeleteAnswer> {
    if elements(text, "DeleteResult").is_empty() {
        return Err(Error::unavailable(
            "object store DeleteObjects answer is not a DeleteResult",
        ));
    }
    let error_codes = elements(text, "Error")
        .into_iter()
        .map(|error| {
            elements(error, "Code")
                .first()
                .map_or_else(|| "Unknown".to_owned(), |code| (*code).to_owned())
        })
        .collect();
    Ok(DeleteAnswer {
        deleted: elements(text, "Deleted").len(),
        error_codes,
    })
}
