use std::process::ExitCode;

use hayamimi_relay::{calendar, cli};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(now) = calendar::now_epoch_secs() else {
        eprintln!("the system clock is set before 1970");
        return ExitCode::from(1);
    };
    let locale = sys_locale::get_locale();
    ExitCode::from(cli::run(&args, now, locale.as_deref()))
}
