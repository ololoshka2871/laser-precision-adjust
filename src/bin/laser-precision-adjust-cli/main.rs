mod cli;

use std::io::Write;

use cli::{parse_cli_command, process_cli_command, CliError};

use laser_precision_adjust::{Error, PrecisionAdjust, Status};
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

    let _monitoring = precision_adjust.start_monitoring().await;
    precision_adjust
        .reset()
        .await
        .expect("Can't reset laser!");

    writeln!(stdout, "Type 'help' to see the list of commands!").unwrap();

    loop {
        tokio::select! {
                status = precision_adjust.get_status() => print_status(status, &mut stdout)?,
                line = rl.readline() => match line {
                Ok(line) => {
                    let line = line.trim();

                    match parse_cli_command(line) {
                        Ok(cmd) => {
                            process_cli_command(&mut precision_adjust, cmd).await;
                            rl.add_history_entry(line.to_owned());
                        },
                        Err(CliError::Parse(e)) => write!(stdout, "\n{}", e)?,
                        Err(CliError::Exit) | Err(CliError::IO(_)) => {
                            writeln!(stdout, "Exiting...")?;
                            return Ok(());
                        }
                    }
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
}

fn print_status(
    status: Result<Status, Error>,
    stdout: &mut impl Write,
) -> Result<(), std::io::Error> {
    use colored::Colorize;
    use laser_setup_interface::{CameraState, ValveState};

    match status {
        Ok(status) => writeln!(
            stdout,
            "[{:0>8.3}]: [{}]; Ch: {}; Step: [{}:{}]; F: {} Hz",
            status.since_start.as_millis() as f32 / 1000.0,
            match (status.camera_state, status.valve_state) {
                (CameraState::Close, ValveState::Atmosphere) => "Closed".green(),
                (CameraState::Close, ValveState::Vacuum) => "Vacuum".red(),
                (CameraState::Open, ValveState::Atmosphere) => "Open".blue(),
                (CameraState::Open, ValveState::Vacuum) => "Open+Vacuum".red().bold(),
            },
            format!("{:02}", status.current_channel).green().bold(),
            format!("{:>2}", status.current_step).purple().bold(),
            format!("{:>5?}", status.current_side).blue(),
            format!("{:0>8.3}", status.current_frequency).yellow()
        ),
        Err(Error::Kosa(kosa_interface::Error::ZeroResponce)) => {
            log::error!(
                "Kosa status channel not initialized! please call start_monitoring() first!"
            );
            Ok(())
        }
        Err(e) => {
            log::error!("Error getting status: {:?}", e);
            Ok(())
        }
    }
}
