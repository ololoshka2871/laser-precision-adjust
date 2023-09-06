mod cli;

use std::io::Write;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use laser_precision_adjust::{PrecisionAdjust, Status};
use rustyline_async::ReadlineError;

use cli::{parse_cli_command, process_cli_command, CliError};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), std::io::Error> {
    let (mut rl, mut stdout) =
        rustyline_async::Readline::new("> ".to_owned()).expect("Failed to init interactive input!");

    // Enable tracing using Tokio's https://tokio.rs/#tk-lib-tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "laser_precision_adjust_server=debug,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    tracing::info!("Loading config...");
    let (config, _) = laser_precision_adjust::Config::load();

    tracing::info!("{}", config);

    if let Some(fifo_name) = config.freq_fifo.as_ref() {
        let s = fifo_name.as_os_str();
        writeln!(
            stdout,
            "Using external fifo to export frequency: {s:?}, connect it to livechart!"
        )?;
    }

    let mut precision_adjust = PrecisionAdjust::with_config(config).await;

    tracing::warn!("Testing connections...");
    if let Err(e) = precision_adjust.test_connection().await {
        panic!("Failed to connect to: {:?}", e);
    } else {
        tracing::info!("Connection successful!");
    }

    let mut status_channel = precision_adjust.start_monitoring().await;
    precision_adjust.reset().await.expect("Can't reset laser!");

    writeln!(stdout, "Type 'help' to see the list of commands!").unwrap();

    loop {
        tokio::select! {
                _changed = status_channel.changed() => {
                    let status = status_channel.borrow();
                    print_status(&status, &mut stdout)?
                }
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

fn print_status(status: &Status, stdout: &mut impl Write) -> Result<(), std::io::Error> {
    use colored::Colorize;
    use laser_setup_interface::{CameraState, ValveState};

    writeln!(
        stdout,
        "[{:0>8.3}]: [{}]; Ch: {}; Step: [{}:{:5}]; F: {} Hz",
        status.since_start.as_millis() as f32 / 1000.0,
        match (status.camera_state, status.valve_state) {
            (CameraState::Close, ValveState::Atmosphere) => "Closed".green(),
            (CameraState::Close, ValveState::Vacuum) => "Vacuum".red(),
            (CameraState::Open, ValveState::Atmosphere) => "Open".blue(),
            (CameraState::Open, ValveState::Vacuum) => "Open+Vacuum".red().bold(),
        },
        format!("{:02}", status.current_channel).green().bold(),
        format!("{:>2}", status.current_step).purple().bold(),
        format!("{:?}", status.current_side).blue(),
        format!("{:0>8.3}", status.current_frequency).yellow()
    )?;

    Ok(())
}
