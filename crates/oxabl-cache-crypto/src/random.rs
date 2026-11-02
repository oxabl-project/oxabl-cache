use crate::error::{Error, Result};

pub(crate) fn fill(buf: &mut [u8]) -> Result<()> {
    aws_lc_rs::rand::fill(buf).map_err(|_| Error::Random)
}

pub(crate) fn array<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    fill(&mut out)?;
    Ok(out)
}
