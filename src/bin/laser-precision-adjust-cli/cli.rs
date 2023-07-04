use clap::{Parser, Subcommand};
use laser_precision_adjust::PrecisionAdjust;

pub enum CliCommand {
    None,
    TestConnection,
    SelectChannel(usize),
}

pub enum CliError {
    Parse,
    Exit,
    IO(std::io::Error),
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        Self::IO(err)
    }
}

#[derive(Parser)]
struct Commands {
    #[command(subcommand)]
    command: Com,
}

/// Doc comment
#[derive(Subcommand)]
//#[clap(global_setting=AppSettings::DisableHelpFlag)]
enum Com {
    /// Exit from the program
    Exit,

    /// Test connections to all devices
    Test,

    /// Select channel to process
    Select {
        #[clap(value_parser=clap::value_parser!(u8).range(0..=16))]
        channel: usize,
    },
}

pub fn parse_cli_command(
    line: &str,
    output: &mut impl std::io::Write,
) -> Result<CliCommand, CliError> {
    let Ok(mut r) = shellwords::split(line) else {
        log::error!("error during process");
        return Err(CliError::Parse);
    };

    r.insert(0, "CLI".to_string());

    if r.is_empty() {
        return Ok(CliCommand::None);
    }

    if r[0] == "help" {
        writeln!(output, "exit - exit the program")?;
        writeln!(output, "help - print this help")?;
        writeln!(output, "test - test connections")?;
        return Ok(CliCommand::None);
    }

    let cmd = Commands::parse_from(r);

    match cmd.command {
        Com::Exit => return Err(CliError::Exit),
        Com::Test => return Ok(CliCommand::TestConnection),
        Com::Select { channel } => return Ok(CliCommand::SelectChannel(channel)),
    }
}

pub async fn process_cli_command(pa: &mut PrecisionAdjust, cmd: CliCommand) {
    match cmd {
        CliCommand::None => {}
        CliCommand::TestConnection => {
            log::info!("Testing connection...");
            if let Err(e) = pa.test_connection().await {
                log::error!("Failed to connect to: {:?}", e);
            } else {
                log::info!("Connection successful!");
            }
        }
        CliCommand::SelectChannel(channel) => {
            log::info!("Selecting channel {}...", channel);
            /*
            if let Err(e) = pa.select_channel(channel).await {
                log::error!("Failed to select channel: {:?}", e);
            } else {
                log::info!("Channel selected!");
            }*/
        }
    }
}