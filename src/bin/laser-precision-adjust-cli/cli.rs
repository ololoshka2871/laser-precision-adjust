use clap::{Parser, Subcommand};
use laser_precision_adjust::PrecisionAdjust;

pub enum CliCommand {
    None,
    TestConnection,
    SelectChannel(u32),
    Open,
    Close(bool),
}

pub enum CliError {
    Parse(String),
    Exit,
    IO(std::io::Error),
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        Self::IO(err)
    }
}

/// Control commands
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
        /// Channel number [0..15]
        #[clap(value_parser=clap::value_parser!(u32).range(0..=16))]
        channel: u32,
    },

    /// Open camera
    Open,

    /// Close camera
    Close,

    /// Vacuum
    Vacuum {
        /// Enable vacuum [true/false]
        #[clap(default_value = "true")]
        on: Option<bool>,
    },
}

pub fn parse_cli_command(line: &str) -> Result<CliCommand, CliError> {
    let Ok(mut r) = shellwords::split(line) else {
        return Err(CliError::Parse("Error during process".to_owned()));
    };

    r.insert(0, "CLI".to_string());

    if r.is_empty() {
        return Ok(CliCommand::None);
    }

    let cmd = Commands::try_parse_from(r);

    match cmd {
        Ok(cmd) => match cmd.command {
            Com::Exit => Err(CliError::Exit),
            Com::Test => Ok(CliCommand::TestConnection),
            Com::Select { channel } => Ok(CliCommand::SelectChannel(channel)),
            Com::Open => Ok(CliCommand::Open),
            Com::Close => Ok(CliCommand::Close(false)),
            Com::Vacuum { on } => Ok(CliCommand::Close(on.unwrap())),
        },
        Err(e) => Err(CliError::Parse(format!("{}", e))),
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
            if let Err(e) = pa.select_channel(channel).await {
                log::error!("Failed to select channel: {:?}", e);
            }
        }
        CliCommand::Open => {
            log::info!("Opening camera...");
            if let Err(e) = pa.open_camera().await {
                log::error!("Failed to open camera: {:?}", e);
            }
        }
        CliCommand::Close(vacuum) => {
            if let Err(e) = pa.close_camera(vacuum).await {
                log::error!("Failed to close camera: {:?}", e);
            }
        }
    }
}
