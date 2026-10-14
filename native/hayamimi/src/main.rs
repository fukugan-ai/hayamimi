use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let locale = sys_locale::get_locale();
    ExitCode::from(hayamimi_app::run::run(&args, locale.as_deref()))
}
