//! zstd with the content checksum on, and a bounded, strict decoder.

use std::io::{self, Read, Write};

use oxabl_cache_format::limits::PACK_BYTES;
use zeroize::Zeroizing;
use zstd::stream::raw::{DParameter, Decoder, Operation};
use zstd::stream::write::Encoder;

use crate::dictionary::Dictionary;
use crate::error::{Error, Result};

const IO_BUFFER: usize = 64 * 1024;
const WINDOW_LOG_MAX: u32 = 27;
const FRAME_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
const CHECKSUM_FLAG: u8 = 0x04;

/// A growing buffer that refuses to hold more than one pack, so a set that cannot be sealed
/// is never buffered.
struct PackBounded(Vec<u8>);

impl Write for PackBounded {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if (self.0.len() + buf.len()) as u64 > PACK_BYTES {
            return Err(io::ErrorKind::OutOfMemory.into());
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encoder_error(e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::OutOfMemory {
        Error::Limit("segment size")
    } else {
        Error::Compression("encoder")
    }
}

/// Compresses everything `input` yields into one frame with a content checksum.
/// Returns the frame and the number of input bytes, failing past `max_input`.
pub(crate) fn compress(
    mut input: impl Read,
    level: i32,
    dictionary: Option<&Dictionary>,
    max_input: u64,
) -> Result<(Zeroizing<Vec<u8>>, u64)> {
    let sink = PackBounded(Vec::with_capacity(IO_BUFFER));
    let mut encoder = match dictionary {
        Some(d) => Encoder::with_prepared_dictionary(sink, d.encoder()),
        None => Encoder::new(sink, level),
    }
    .map_err(|_| Error::Compression("encoder"))?;
    encoder
        .include_checksum(true)
        .map_err(|_| Error::Compression("encoder"))?;
    let mut buf = Zeroizing::new(vec![0u8; IO_BUFFER]);
    let mut total = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max_input {
            return Err(Error::Limit("plaintext per artifact set"));
        }
        encoder.write_all(&buf[..n]).map_err(encoder_error)?;
    }
    let frame = encoder.finish().map_err(encoder_error)?;
    Ok((Zeroizing::new(frame.0), total))
}

fn check_frame_header(frame: &[u8]) -> Result<()> {
    if frame.len() < 6 || frame[..4] != FRAME_MAGIC {
        return Err(Error::Compression("not a zstd frame"));
    }
    if frame[4] & CHECKSUM_FLAG == 0 {
        return Err(Error::Compression("frame has no content checksum"));
    }
    Ok(())
}

/// Decompresses exactly one frame that spans `frame` and yields exactly `plain_len` bytes,
/// with its checksum verified and its window within 128 MiB.
pub(crate) fn decompress(
    frame: &[u8],
    dictionary: Option<&Dictionary>,
    plain_len: u64,
    mut out: impl Write,
) -> Result<()> {
    check_frame_header(frame)?;
    let mut decoder = match dictionary {
        Some(d) => Decoder::with_prepared_dictionary(d.decoder()),
        None => Decoder::new(),
    }
    .map_err(|_| Error::Compression("decoder"))?;
    decoder
        .set_parameter(DParameter::WindowLogMax(WINDOW_LOG_MAX))
        .map_err(|_| Error::Compression("decoder"))?;
    let mut buf = Zeroizing::new(vec![0u8; IO_BUFFER * 2]);
    let (mut read, mut written) = (0usize, 0u64);
    loop {
        let status = decoder
            .run_on_buffers(&frame[read..], &mut buf)
            .map_err(|_| Error::Compression("corrupt frame"))?;
        read += status.bytes_read;
        written += status.bytes_written as u64;
        if written > plain_len {
            return Err(Error::Compression("output exceeds the signed length"));
        }
        out.write_all(&buf[..status.bytes_written])?;
        if status.remaining == 0 {
            break;
        }
        if status.bytes_read == 0 && status.bytes_written == 0 {
            return Err(Error::Compression("truncated frame"));
        }
    }
    if read != frame.len() {
        return Err(Error::Compression("trailing bytes after the frame"));
    }
    if written != plain_len {
        return Err(Error::Compression(
            "output is shorter than the signed length",
        ));
    }
    Ok(())
}
