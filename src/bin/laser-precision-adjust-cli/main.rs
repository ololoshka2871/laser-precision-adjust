mod cli;

use std::io::Write;

use cli::{process_command, CliError};

use laser_precision_adjust::PrecisionAdjust;
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

    let mut precision_adjust = PrecisionAdjust::with_config(config);

    log::warn!("Testing connections...");
    if let Err(e) = precision_adjust.test_connection().await {
        panic!("Failed to connect to: {:?}", e);
    } else {
        log::info!("Connection successful!");
    }

    writeln!(stdout, "Type 'help' to see the list of commands!").unwrap();

    loop {
        match rl.readline().await {
            Ok(line) => {
                let line = line.trim();

                match process_command(line, &mut stdout) {
                    Ok(cmd) => process_cli_command(&mut precision_adjust, cmd).await,
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

async fn process_cli_command(pa: &mut PrecisionAdjust, cmd: cli::CliCommand) {
    match cmd {
        cli::CliCommand::None => {}
        cli::CliCommand::TestConnection => {
            log::info!("Testing connection...");
            if let Err(e) = pa.test_connection().await {
                log::error!("Failed to connect to: {:?}", e);
            } else {
                log::info!("Connection successful!");
            }
        }
    }
}