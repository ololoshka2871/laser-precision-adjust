
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), ()> {
    env_logger::init();

    log::info!("Loading config...");
    let _config = laser_precision_adjust::config::Config::load();
    
    Ok(())
}