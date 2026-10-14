use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let locale = sys_locale::get_locale();
    let code = match args.split_first() {
        Some((command, rest)) if command == "llama" => {
            hayamimi_app::llama::run(rest, locale.as_deref())
        }
        _ => hayamimi_app::run::run(&args, locale.as_deref()),
    };
    ExitCode::from(code)
}
