//! SubRip (SRT) cues: the text of one subtitle with its span of speech, and a
//! writer that appends cues to any output.
//!
//! A cue is flushed as it is written, so a run that ends abruptly keeps every
//! cue before the end.

use std::io::{self, Write};

/// One subtitle: a span of speech in seconds and the text shown over it.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start_s: f64,
    pub end_s: f64,
    pub text: String,
}

impl Cue {
    /// A cue for a span that runs forward from a time that is not negative,
    /// with text that is not blank. `None` for any other input. Blank lines
    /// inside the text are dropped, because a blank line ends a cue.
    pub fn new(start_s: f64, end_s: f64, text: &str) -> Option<Cue> {
        let forward = start_s.is_finite() && end_s.is_finite() && start_s >= 0.0 && end_s >= start_s;
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if !forward || lines.is_empty() {
            return None;
        }
        Some(Cue {
            start_s,
            end_s,
            text: lines.join("\n"),
        })
    }
}

/// `HH:MM:SS,mmm` of a time in seconds, rounded to the millisecond.
pub fn timestamp(seconds: f64) -> String {
    let millis_total = (seconds * 1000.0).round() as u64;
    let (hours, rest) = (millis_total / 3_600_000, millis_total % 3_600_000);
    let (minutes, rest) = (rest / 60_000, rest % 60_000);
    let (secs, millis) = (rest / 1000, rest % 1000);
    format!("{hours:02}:{minutes:02}:{secs:02},{millis:03}")
}

/// The text of the cue numbered `index`, with the blank line that ends it.
pub fn render(index: usize, cue: &Cue) -> String {
    format!(
        "{index}\n{} --> {}\n{}\n\n",
        timestamp(cue.start_s),
        timestamp(cue.end_s),
        cue.text
    )
}

/// Appends numbered cues to an output.
pub struct SrtWriter {
    out: Box<dyn Write + Send>,
    written: usize,
}

impl SrtWriter {
    pub fn new(out: Box<dyn Write + Send>) -> SrtWriter {
        SrtWriter { out, written: 0 }
    }

    /// Writes the next cue and flushes it. The cue counts only when it was
    /// written whole.
    pub fn push(&mut self, cue: &Cue) -> io::Result<()> {
        let next = self.written + 1;
        self.out.write_all(render(next, cue).as_bytes())?;
        self.out.flush()?;
        self.written = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// An output the test can read back.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            match self.0.lock() {
                Ok(mut inner) => inner.extend_from_slice(bytes),
                Err(poisoned) => poisoned.into_inner().extend_from_slice(bytes),
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Shared {
        fn text(&self) -> String {
            match self.0.lock() {
                Ok(inner) => String::from_utf8_lossy(&inner).into_owned(),
                Err(poisoned) => String::from_utf8_lossy(&poisoned.into_inner()).into_owned(),
            }
        }
    }

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("disk full"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_timestamp_has_hours_minutes_seconds_and_milliseconds() {
        assert_eq!(timestamp(0.0), "00:00:00,000");
        assert_eq!(timestamp(3661.5), "01:01:01,500");
        assert_eq!(timestamp(59.9996), "00:01:00,000");
        assert_eq!(timestamp(0.0004), "00:00:00,000");
        assert_eq!(timestamp(360_000.25), "100:00:00,250");
    }

    #[test]
    fn a_cue_needs_a_forward_span_and_some_text() {
        assert!(Cue::new(1.0, 2.0, "x").is_some());
        assert!(Cue::new(1.0, 1.0, "x").is_some());
        assert!(Cue::new(2.0, 1.0, "x").is_none());
        assert!(Cue::new(-0.1, 1.0, "x").is_none());
        assert!(Cue::new(f64::NAN, 1.0, "x").is_none());
        assert!(Cue::new(0.0, f64::INFINITY, "x").is_none());
        assert!(Cue::new(0.0, 1.0, " \n ").is_none());
    }

    #[test]
    fn a_blank_line_inside_the_text_is_dropped() {
        let cue = Cue::new(0.0, 1.0, "one\n\n  \ntwo ");
        assert_eq!(cue.map(|found| found.text), Some(String::from("one\ntwo")));
    }

    #[test]
    fn cues_are_numbered_from_one_and_end_with_a_blank_line() -> io::Result<()> {
        let out = Shared::default();
        let mut writer = SrtWriter::new(Box::new(out.clone()));
        for (start, text) in [(0.5, "Hello."), (2.0, "Bye.")] {
            let cue = Cue::new(start, start + 1.0, text)
                .ok_or_else(|| io::Error::other("a cue the test wrote was refused"))?;
            writer.push(&cue)?;
        }
        assert_eq!(
            out.text(),
            "1\n00:00:00,500 --> 00:00:01,500\nHello.\n\n2\n00:00:02,000 --> 00:00:03,000\nBye.\n\n"
        );
        Ok(())
    }

    #[test]
    fn a_cue_that_could_not_be_written_is_not_counted() -> io::Result<()> {
        let cue = Cue::new(0.0, 1.0, "x")
            .ok_or_else(|| io::Error::other("a cue the test wrote was refused"))?;
        let mut writer = SrtWriter::new(Box::new(Broken));
        assert!(writer.push(&cue).is_err());
        assert_eq!(writer.written, 0);
        Ok(())
    }
}
