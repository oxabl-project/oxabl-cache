//! Domain-separation strings of the key hierarchy and of every signed or authenticated transcript.

pub(crate) const CTX_LOOKUP: &str = "oxabl-cache v1 lookup";
pub(crate) const CTX_SEGMENT: &str = "oxabl-cache v1 segment";
pub(crate) const CTX_RECORD: &str = "oxabl-cache v1 record";
pub(crate) const CTX_ENC: &str = "oxabl-cache v1 enc";
pub(crate) const CTX_COMMIT: &str = "oxabl-cache v1 commit";

pub(crate) const AD_RECORD: &[u8] = b"oxabl-cache v1 record";
pub(crate) const SIG_ENTRY: &[u8] = b"oxabl-cache v1 entry";
pub(crate) const SIG_ROSTER: &[u8] = b"oxabl-cache v1 roster";
pub(crate) const SIG_DICTIONARY: &[u8] = b"oxabl-cache v1 dictionary";

pub(crate) fn prefixed(prefix: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + body.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(body);
    out
}
