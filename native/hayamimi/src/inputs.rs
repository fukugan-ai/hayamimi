//! The command-line options that say where the audio comes from: the kind of
//! input, the devices to open and the network endpoint `/ingest`.

use std::net::{IpAddr, Ipv4Addr};

use crate::args::ArgError;

/// The port `/ingest` listens on when `--input ws` has no `--ws-port`.
pub const DEFAULT_INGEST_PORT: u16 = 8766;

/// Where the audio of a run comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A wav file given with `--wav`.
    Wav,
    /// The default or the named microphone.
    Mic,
    /// What the default or the named output device plays (Windows).
    Speaker,
    /// The microphone and the output device added together.
    Mix,
    /// Clients that send audio to `/ingest`.
    Ws,
}

impl Input {
    /// Reads the value of `--input`.
    pub fn parse(text: &str) -> Option<Input> {
        match text {
            "wav" => Some(Input::Wav),
            "mic" => Some(Input::Mic),
            "speaker" => Some(Input::Speaker),
            "mix" => Some(Input::Mix),
            "ws" => Some(Input::Ws),
            _ => None,
        }
    }

    /// Whether the audio comes from a device of this machine.
    pub fn captures(self) -> bool {
        matches!(self, Input::Mic | Input::Speaker | Input::Mix)
    }
}

/// The options of the audio input.
#[derive(Debug, Clone, PartialEq)]
pub struct IoArgs {
    /// `--input`; without it the input is the wav file.
    pub input: Option<Input>,
    pub mic_device: Option<String>,
    pub speaker_device: Option<String>,
    pub ws_host: IpAddr,
    /// `--ws-port`; `/ingest` listens when this or `--input ws` is given.
    pub ws_port: Option<u16>,
    /// Drafts of the utterance in progress; `--no-partial` turns them off.
    pub partial: bool,
}

impl Default for IoArgs {
    fn default() -> Self {
        IoArgs {
            input: None,
            mic_device: None,
            speaker_device: None,
            ws_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            ws_port: None,
            partial: true,
        }
    }
}

/// The arguments not yet read.
type Rest<'a> = std::iter::Peekable<std::slice::Iter<'a, String>>;

fn value_of<'a>(rest: &mut Rest<'a>) -> Result<&'a String, ArgError> {
    rest.next().ok_or(ArgError::MissingValue)
}

impl IoArgs {
    /// Reads one option of this group and the value it takes. An option of
    /// another group is an unknown option here.
    pub fn apply(&mut self, option: &str, rest: &mut Rest<'_>) -> Result<(), ArgError> {
        match option {
            "--input" => {
                self.input = Some(Input::parse(value_of(rest)?).ok_or(ArgError::UnknownInput)?);
            }
            "--mic-device" => self.mic_device = Some(value_of(rest)?.clone()),
            "--speaker-device" => self.speaker_device = Some(value_of(rest)?.clone()),
            "--ws-host" => {
                self.ws_host = value_of(rest)?
                    .parse::<IpAddr>()
                    .map_err(|_| ArgError::BadAddress)?;
            }
            "--ws-port" => {
                self.ws_port = Some(
                    value_of(rest)?
                        .parse::<u16>()
                        .map_err(|_| ArgError::BadNumber)?,
                );
            }
            "--no-partial" => self.partial = false,
            _ => return Err(ArgError::UnknownOption),
        }
        Ok(())
    }

    /// The input of the run: the one asked for, else the wav file.
    pub fn kind(&self) -> Input {
        // DEFAULT: a command line without --input reads the file named by --wav, as the usage text says.
        self.input.unwrap_or(Input::Wav)
    }

    /// The port `/ingest` listens on, or `None` when it does not listen.
    pub fn ingest_port(&self) -> Option<u16> {
        match (self.ws_port, self.kind()) {
            (Some(port), _) => Some(port),
            (None, Input::Ws) => Some(DEFAULT_INGEST_PORT),
            (None, _) => None,
        }
    }

    /// Whether the run has an input at all once `--wav` is known.
    pub fn is_complete(&self, has_wav: bool) -> bool {
        match self.kind() {
            Input::Wav => has_wav,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn read(items: &[&str]) -> Result<IoArgs, ArgError> {
        let items = list(items);
        let mut rest = items.iter().peekable();
        let mut io = IoArgs::default();
        while let Some(option) = rest.next() {
            io.apply(option, &mut rest)?;
        }
        Ok(io)
    }

    #[test]
    fn every_kind_of_input_is_named() {
        for (name, kind) in [
            ("wav", Input::Wav),
            ("mic", Input::Mic),
            ("speaker", Input::Speaker),
            ("mix", Input::Mix),
            ("ws", Input::Ws),
        ] {
            assert_eq!(Input::parse(name), Some(kind));
        }
        assert_eq!(Input::parse("line"), None);
    }

    #[test]
    fn only_the_device_kinds_capture() {
        assert!(Input::Mic.captures() && Input::Speaker.captures() && Input::Mix.captures());
        assert!(!Input::Wav.captures() && !Input::Ws.captures());
    }

    #[test]
    fn nothing_given_is_the_wav_file_without_ingest() {
        let io = IoArgs::default();
        assert_eq!(io.kind(), Input::Wav);
        assert_eq!(io.ingest_port(), None);
        assert!(io.partial);
        assert_eq!(io.ws_host, IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn the_ws_input_listens_on_the_default_port_unless_told_otherwise() -> Result<(), ArgError> {
        assert_eq!(
            read(&["--input", "ws"])?.ingest_port(),
            Some(DEFAULT_INGEST_PORT)
        );
        assert_eq!(
            read(&["--input", "ws", "--ws-port", "9100"])?.ingest_port(),
            Some(9100)
        );
        Ok(())
    }

    #[test]
    fn a_ws_port_starts_ingest_beside_another_input() -> Result<(), ArgError> {
        let io = read(&["--input", "mic", "--ws-port", "8800"])?;
        assert_eq!(io.kind(), Input::Mic);
        assert_eq!(io.ingest_port(), Some(8800));
        Ok(())
    }

    #[test]
    fn the_devices_and_the_host_are_read() -> Result<(), ArgError> {
        let io = read(&[
            "--mic-device",
            "USB",
            "--speaker-device",
            "Realtek",
            "--ws-host",
            "0.0.0.0",
            "--no-partial",
        ])?;
        assert_eq!(io.mic_device.as_deref(), Some("USB"));
        assert_eq!(io.speaker_device.as_deref(), Some("Realtek"));
        assert_eq!(io.ws_host, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert!(!io.partial);
        Ok(())
    }

    #[test]
    fn a_bad_value_names_what_is_wrong() {
        assert_eq!(read(&["--input", "line"]), Err(ArgError::UnknownInput));
        assert_eq!(read(&["--ws-host", "host"]), Err(ArgError::BadAddress));
        assert_eq!(read(&["--ws-port", "70000"]), Err(ArgError::BadNumber));
        assert_eq!(read(&["--ws-port", "x"]), Err(ArgError::BadNumber));
        assert_eq!(read(&["--mic-device"]), Err(ArgError::MissingValue));
        assert_eq!(read(&["--other"]), Err(ArgError::UnknownOption));
    }

    #[test]
    fn a_wav_run_needs_its_file_and_a_device_run_does_not() -> Result<(), ArgError> {
        assert!(!IoArgs::default().is_complete(false));
        assert!(IoArgs::default().is_complete(true));
        assert!(read(&["--input", "mic"])?.is_complete(false));
        assert!(read(&["--input", "ws"])?.is_complete(false));
        assert!(!read(&["--input", "wav"])?.is_complete(false));
        Ok(())
    }
}
