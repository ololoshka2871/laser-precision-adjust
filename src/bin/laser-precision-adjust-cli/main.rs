mod cli;

use std::io::Write;

use cli::{process_command, CliError};

use rustyline_async::ReadlineError;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), std::io::Error> {
    let (mut rl, mut stdout) =
        rustyline_async::Readline::new("> ".to_owned()).expect("Failed to init interactive input!");

    env_logger::builder()
        .format_timestamp(None)
        .parse_default_env()
        .target(env_logger::Target::Stderr)
        .init();

    log::info!("Loading config...");
    let config = laser_precision_adjust::Config::load();

    log::info!("{}", config);

    let mut _precision_adjust = laser_precision_adjust::PrecisionAdjust::with_config(config);

    writeln!(stdout, "Type 'help' to see the list of commands!").unwrap();

    loop {
        match rl.readline().await {
            Ok(line) => {
                let line = line.trim();

                match process_command(line, &mut stdout) {
                    Ok(()) => {}
                    Err(CliError::Parse) => continue,
                    Err(CliError::Exit) | Err(CliError::IO(_)) => {
                        writeln!(stdout, "Exiting...")?;
                        return Ok(());
                    }
                }

                //
            }
            Err(ReadlineError::Eof) | Err(ReadlineError::Closed) => {
                writeln!(stdout, "Exiting...")?;
                return Ok(());
            }
            Err(ReadlineError::Interrupted) => {
                writeln!(stdout, "^C")?;
                return Ok(());
            }
            Err(ReadlineError::IO(err)) => {
                writeln!(stdout, "Received err: {:?}", err)?;
                return Err(err);
            }
        }
    }
}
