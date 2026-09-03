use std::process::ExitCode;

fn main() -> ExitCode {
    match zenpi::core::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zenpi: {error}");
            ExitCode::FAILURE
        }
    }
}
