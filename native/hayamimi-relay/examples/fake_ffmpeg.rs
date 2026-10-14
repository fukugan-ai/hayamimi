//! A stand-in for `ffmpeg` in the tests of `media`: it copies the input named
//! after `-i` to the output, which is the last argument. A missing input ends
//! with code 1, as ffmpeg does. A test that puts events where a video would be
//! gets them back as the "audio", for the stand-in recognizer to serve.

use std::process::ExitCode;

fn run(args: &[String]) -> Result<(), String> {
    let at = args.iter().position(|arg| arg == "-i").ok_or("no -i")?;
    let input = args.get(at + 1).ok_or("no input")?;
    let output = args.last().ok_or("no output")?;
    std::fs::copy(input, output)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("fake ffmpeg: {reason}");
            ExitCode::from(1)
        }
    }
}
