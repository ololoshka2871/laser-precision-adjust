
pub enum CliCommand {
    None,
    TestConnection,
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

pub fn process_command(line: &str, output: &mut impl std::io::Write) -> Result<CliCommand, CliError> {
    if line.is_empty() {
        return Ok(CliCommand::None);
    }

    if line == "exit" {
        return Err(CliError::Exit);
    }

    if line == "help" {
        writeln!(output, "exit - exit the program")?;
        writeln!(output, "help - print this help")?;
        writeln!(output, "test - test connections")?;
        return Ok(CliCommand::None);
    }

    if line == "test" {
        return Ok(CliCommand::TestConnection);
    }

    /*
    if line == "adjust" {
        log::info!("Adjusting the laser...");
        _precision_adjust.adjust().await?;
        log::info!("Laser adjusted!");
        continue;
    }

    if line == "status" {
        log::info!("Printing the status of the laser...");
        _precision_adjust.print_status().await?;
        log::info!("Status printed!");
        continue;
    }

    if line == "config" {
        log::info!("Printing the config...");
        writeln!(stdout, "{}", _precision_adjust.config)?;
        log::info!("Config printed!");
        continue;
    }
    */
    println!("Unknown command! Type 'help' to see the list of commands!");
    Err(CliError::Parse)
}
