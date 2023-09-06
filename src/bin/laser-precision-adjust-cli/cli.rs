use clap::{Parser, Subcommand};
use laser_precision_adjust::PrecisionAdjust;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum CliCommand {
    None,
    TestConnection,
    SelectChannel(u32),
    Open,
    Close(bool),
    Step(i32),
    Burn(Option<i32>),
    Show {
        burn: bool,
        pump: Option<f32>,
        s: Option<f32>,
        f: Option<f32>,
    },
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
    #[clap(alias = "quit")]
    Exit,

    /// Test connections to all devices
    Test,

    /// Select channel to process
    #[clap(alias = "sel")]
    Select {
        /// Channel number [0..15]
        #[clap(value_parser=clap::value_parser!(u32).range(0..=16))]
        channel: u32,
    },

    /// Open camera
    #[clap(alias = "o")]
    Open,

    /// Close camera
    #[clap(alias = "c")]
    Close,

    /// Vacuum
    #[clap(alias = "vac")]
    Vacuum {
        /// Enable vacuum [true/false]
        #[clap(default_value = "true")]
        on: Option<bool>,
    },

    /// Perform vertical step
    #[clap(alias = "s")]
    Step {
        #[clap(default_value = "1")]
        count: Option<i32>,
    },

    /// Perform horisontal burn step
    #[clap(alias = "b")]
    Burn {
        /// Autostep
        #[clap(default_value = "0")]
        autostep: Option<i32>,
    },

    /// Show current working area
    Show {
        /// enable burning
        #[clap(short, long)]
        burn: bool,

        /// Override pump power
        #[clap(short, long)]
        pump: Option<f32>,

        /// Override laser power
        #[clap(short, long)]
        s: Option<f32>,

        /// Override feedrate
        #[clap(short, long)]
        f: Option<f32>,
    },
}

pub fn parse_cli_command(line: &str) -> Result<CliCommand, CliError> {
    static mut LAST_CMD: CliCommand = CliCommand::None;

    let Ok(mut r) = shellwords::split(line) else {
        return Err(CliError::Parse("Error during process".to_owned()));
    };

    r.insert(0, "CLI".to_string());

    if r.len() == 1 {
        // Return last command
        return Ok(unsafe { LAST_CMD });
    }

    let cmd = Commands::try_parse_from(r);

    match cmd {
        Ok(cmd) => {
            let new_cmd = match cmd.command {
                Com::Exit => return Err(CliError::Exit),
                Com::Test => Ok(CliCommand::TestConnection),
                Com::Select { channel } => Ok(CliCommand::SelectChannel(channel)),
                Com::Open => Ok(CliCommand::Open),
                Com::Close => Ok(CliCommand::Close(false)),
                Com::Vacuum { on } => Ok(CliCommand::Close(on.unwrap())),
                Com::Step { count } => Ok(CliCommand::Step(count.unwrap())),
                Com::Burn { autostep } => Ok(CliCommand::Burn(autostep)),
                Com::Show { burn, pump, s, f } => Ok(CliCommand::Show { burn, pump, s, f }),
            };
            unsafe {
                LAST_CMD = new_cmd.as_ref().unwrap_or(&CliCommand::None).clone();
            }
            new_cmd
        }
        Err(e) => Err(CliError::Parse(format!("{}", e))),
    }
}

pub async fn process_cli_command(pa: &mut PrecisionAdjust, cmd: CliCommand) {
    match cmd {
        CliCommand::None => {}
        CliCommand::TestConnection => {
            tracing::info!("Testing connection...");
            if let Err(e) = pa.test_connection().await {
                tracing::error!("Failed to connect to: {:?}", e);
            } else {
                tracing::info!("Connection successful!");
            }
        }
        CliCommand::SelectChannel(channel) => {
            tracing::info!("Selecting channel {}...", channel);
            if let Err(e) = pa.select_channel(channel).await {
                tracing::error!("Failed to select channel: {:?}", e);
            }
        }
        CliCommand::Open => {
            tracing::info!("Opening camera...");
            if let Err(e) = pa.open_camera().await {
                tracing::error!("Failed to open camera: {:?}", e);
            }
        }
        CliCommand::Close(vacuum) => {
            if let Err(e) = pa.close_camera(vacuum).await {
                tracing::error!("Failed to close camera: {:?}", e);
            }
        }
        CliCommand::Step(count) => {
            if let Err(e) = pa.step(count).await {
                tracing::error!("Failed to perform step: {:?}", e);
            }
        }
        CliCommand::Burn(autostep) => {
            if let Err(e) = pa.burn().await {
                tracing::error!("Failed to perform burn: {:?}", e);
            }
            if let Some(autostep) = autostep {
                if let Err(e) = pa.step(autostep).await {
                    tracing::error!("Failed to perform step: {:?}", e);
                }
            }
        }
        CliCommand::Show { burn, pump, s, f } => {
            if let Err(e) = pa.show(burn, pump, s, f).await {
                tracing::error!("Failed to show: {:?}", e);
            }
        }
    }
}
