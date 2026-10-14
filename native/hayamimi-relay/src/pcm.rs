//! The samples of a wav file, read a frame's worth at a time.
//!
//! Only the file `ffmpeg` is asked for is understood: 16 kHz, one channel,
//! 16-bit PCM. Any other format is refused rather than sent as if it were.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const SAMPLE_RATE: u32 = 16_000;

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

/// The data chunk of a wav file, positioned at its first byte.
pub struct Pcm<R> {
    reader: R,
    left: u64,
}

/// Opens the wav file at `path`.
pub fn open(path: &Path) -> io::Result<Pcm<BufReader<File>>> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    Pcm::new(BufReader::new(file), length)
}

fn read_array<const N: usize, R: Read>(reader: &mut R) -> io::Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Checks the body of a `fmt ` chunk.
fn check_format(body: &[u8]) -> io::Result<()> {
    let [f0, f1, c0, c1, r0, r1, r2, r3, _, _, _, _, _, _, b0, b1, ..] = *body else {
        return Err(invalid("a fmt chunk that is too short"));
    };
    let format = u16::from_le_bytes([f0, f1]);
    let channels = u16::from_le_bytes([c0, c1]);
    let rate = u32::from_le_bytes([r0, r1, r2, r3]);
    let bits = u16::from_le_bytes([b0, b1]);
    if (format, channels, rate, bits) == (1, 1, SAMPLE_RATE, 16) {
        Ok(())
    } else {
        Err(invalid("not 16 kHz mono 16-bit PCM"))
    }
}

impl<R: Read + Seek> Pcm<R> {
    /// Reads the chunks up to `data`. `length` is the file's size, which
    /// bounds a data chunk whose size field is wrong.
    pub fn new(mut reader: R, length: u64) -> io::Result<Pcm<R>> {
        let head: [u8; 12] = read_array(&mut reader)?;
        if &head[..4] != b"RIFF" || &head[8..] != b"WAVE" {
            return Err(invalid("not a RIFF/WAVE file"));
        }
        let mut formatted = false;
        loop {
            let chunk: [u8; 8] = read_array(&mut reader)?;
            let size = u64::from(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
            match &chunk[..4] {
                b"fmt " => {
                    let mut body = vec![0_u8; size.min(256) as usize];
                    reader.read_exact(&mut body)?;
                    check_format(&body)?;
                    formatted = true;
                    skip(&mut reader, size - body.len() as u64 + (size & 1))?;
                }
                b"data" if formatted => {
                    let start = reader.stream_position()?;
                    let left = size.min(length.saturating_sub(start));
                    return Ok(Pcm {
                        reader,
                        left: left & !1,
                    });
                }
                b"data" => return Err(invalid("a data chunk before the fmt chunk")),
                _ => skip(&mut reader, size + (size & 1))?,
            }
        }
    }

    /// Reads up to `buf.len()` bytes of samples; 0 at the end of the data.
    pub fn fill(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let want = (buf.len() as u64).min(self.left) as usize;
        self.reader.read_exact(&mut buf[..want])?;
        self.left -= want as u64;
        Ok(want)
    }
}

fn skip<R: Seek>(reader: &mut R, bytes: u64) -> io::Result<()> {
    let bytes = i64::try_from(bytes).map_err(|_| invalid("a chunk that large"))?;
    reader.seek(SeekFrom::Current(bytes)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use io::Cursor;

    /// A wav file: `extra` chunk bytes after the format, then the data.
    fn wav(format: [u16; 2], rate: u32, extra: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&format[0].to_le_bytes());
        out.extend_from_slice(&format[1].to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2_u16.to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(extra);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    fn pcm(bytes: Vec<u8>) -> io::Result<Pcm<Cursor<Vec<u8>>>> {
        let length = bytes.len() as u64;
        Pcm::new(Cursor::new(bytes), length)
    }

    fn drain(mut pcm: Pcm<Cursor<Vec<u8>>>, frame: usize) -> io::Result<Vec<usize>> {
        let mut sizes = Vec::new();
        let mut buf = vec![0_u8; frame];
        loop {
            match pcm.fill(&mut buf)? {
                0 => return Ok(sizes),
                n => sizes.push(n),
            }
        }
    }

    #[test]
    fn the_data_is_read_in_frames_and_the_last_one_is_short() -> io::Result<()> {
        let bytes = wav([1, 1], 16_000, &[], &[1; 10]);
        assert_eq!(drain(pcm(bytes)?, 4)?, [4, 4, 2]);
        Ok(())
    }

    #[test]
    fn a_chunk_between_the_format_and_the_data_is_skipped() -> io::Result<()> {
        let mut extra = b"LIST".to_vec();
        extra.extend_from_slice(&3_u32.to_le_bytes());
        extra.extend_from_slice(&[7, 7, 7, 0]);
        let bytes = wav([1, 1], 16_000, &extra, &[1; 6]);
        assert_eq!(drain(pcm(bytes)?, 64)?, [6]);
        Ok(())
    }

    #[test]
    fn a_wav_with_no_samples_has_no_frames() -> io::Result<()> {
        assert!(drain(pcm(wav([1, 1], 16_000, &[], &[]))?, 4)?.is_empty());
        Ok(())
    }

    #[test]
    fn a_data_size_larger_than_the_file_stops_at_the_file() -> io::Result<()> {
        let mut bytes = wav([1, 1], 16_000, &[], &[1; 8]);
        let at = bytes.len() - 8 - 4;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(drain(pcm(bytes)?, 64)?, [8]);
        Ok(())
    }

    #[test]
    fn another_format_is_refused() {
        for bytes in [
            wav([1, 2], 16_000, &[], &[0; 4]),
            wav([1, 1], 44_100, &[], &[0; 4]),
            wav([3, 1], 16_000, &[], &[0; 4]),
        ] {
            assert!(pcm(bytes).is_err());
        }
    }

    #[test]
    fn something_that_is_not_a_wav_is_refused() {
        assert!(pcm(b"{\"type\":\"final\"} and more text".to_vec()).is_err());
        assert!(pcm(Vec::new()).is_err());
    }
}
