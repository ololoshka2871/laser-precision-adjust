pub mod auto;
pub mod common;
pub mod config;
pub mod handle_control;
pub mod handle_stat;
pub mod into_body;
pub mod limits;
pub mod manual;
pub mod static_files;

pub(crate) use auto::{
    handle_auto_adjust, handle_auto_adjust_status, handle_generate_report_excel,
};
pub(crate) use config::{handle_config, handle_update_config, handle_config_and_save};
pub(crate) use handle_control::handle_control;
pub(crate) use handle_stat::{
    handle_stat_auto, handle_stat_manual, handle_stat_rez_auto, handle_stat_rez_manual,
};
pub(crate) use manual::{handle_generate_report, handle_state, handle_work};
