//! Running external programs. Every child is waited for before the function
//! returns, so no process outlives the task.

use std::process::Command;

use crate::error::Error;

fn program_of(command: &Command) -> String {
    command.get_program().to_string_lossy().into_owned()
}

/// Runs the command with inherited output and waits for it to finish.
pub fn run(command: &mut Command) -> Result<(), Error> {
    let program = program_of(command);
    let status = command.status().map_err(|source| Error::Spawn {
        program: program.clone(),
        source,
    })?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Failed {
        program,
        code: status.code(),
    })
}

/// Runs the command, waits for it and returns what it printed on standard output.
pub fn capture(command: &mut Command) -> Result<String, Error> {
    let program = program_of(command);
    let output = command.output().map_err(|source| Error::Spawn {
        program: program.clone(),
        source,
    })?;
    if !output.status.success() {
        return Err(Error::Failed {
            program,
            code: output.status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_does_not_exist_is_a_spawn_error() {
        let said = run(&mut Command::new("hayamimi-no-such-program"));
        assert!(matches!(said, Err(Error::Spawn { .. })), "{said:?}");
    }

    #[cfg(windows)]
    #[test]
    fn a_nonzero_status_is_reported_with_its_code() {
        let said = run(Command::new("cmd").args(["/C", "exit 3"]));
        assert!(
            matches!(said, Err(Error::Failed { code: Some(3), .. })),
            "{said:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn captured_output_is_returned_as_text() -> Result<(), Error> {
        let said = capture(Command::new("cmd").args(["/C", "echo hayamimi"]))?;
        assert_eq!(said.trim(), "hayamimi");
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn a_capture_of_a_failing_program_is_an_error() {
        let said = capture(Command::new("cmd").args(["/C", "exit 1"]));
        assert!(
            matches!(said, Err(Error::Failed { code: Some(1), .. })),
            "{said:?}"
        );
    }
}
