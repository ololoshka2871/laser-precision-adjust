use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Local};
use laser_precision_adjust::{box_plot::BoxPlot, AutoAdjustLimits};
use serde::Serialize;
use tokio::sync::{watch, Mutex};

use crate::{
    auto_adjust_single_controller::HardwareLogickError,
    far_long_iterator::{FarLongIterator, FarLongIteratorItem, IntoFarLongIterator},
};

#[derive(Clone, Copy, Debug)]
pub enum ChannelState {
    InProcess,
    Unsatable,
    OutOfRange,
    NoReaction,
    Ok,
}

impl ChannelState {
    fn to_status_icon(&self) -> String {
        match self {
            ChannelState::InProcess => "-",
            ChannelState::Unsatable => "x",
            ChannelState::OutOfRange => "↕",
            ChannelState::NoReaction => "∙",
            ChannelState::Ok => "ⓥ",
        }
        .to_owned()
    }
}

#[derive(Clone)]
pub struct ChannelRef {
    id: usize,
    last_selected: DateTime<Local>,
    total_channels: usize,
    state: ChannelState,
    current_freq: f32,
    current_step: u32,
}

impl ChannelRef {
    pub fn new(id: usize, total_channels: usize) -> Self {
        Self {
            id,
            last_selected: Local::now(),
            total_channels,
            state: ChannelState::InProcess,
            current_freq: 0.0,
            current_step: 0,
        }
    }

    fn touch(&mut self) {
        self.last_selected = Local::now();
    }

    fn update_state(&mut self, state: ChannelState, freq: f32, step: u32) {
        tracing::debug!(
            "Rezonator {} update state {:?}: F={}, step={}",
            self.id,
            state,
            freq,
            step
        );
        self.state = state;
        self.current_freq = freq;
        self.current_step = step;
    }

    fn get_state(&self) -> ChannelState {
        self.state
    }
}

impl FarLongIteratorItem for ChannelRef {
    fn distance(&self, other: &Self) -> u64 {
        let forward_distance = if self.id > other.id {
            self.id - other.id
        } else {
            other.id - self.id
        };
        let wraped_distance = if self.id > other.id {
            other.id + self.total_channels - self.id
        } else {
            self.id + self.total_channels - other.id
        };

        std::cmp::min(forward_distance, wraped_distance) as u64
    }

    fn last_selected(&self) -> DateTime<Local> {
        self.last_selected
    }

    fn is_valid(&self) -> bool {
        match self.state {
            ChannelState::InProcess => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Error {
    AdjustInProgress,
    NothingToCancel,
}

#[derive(Debug, Clone, Serialize)]
pub enum ProgressStatus {
    /// Бездействие
    Idle,

    /// Поиск края
    SearchingEdge { ch: u32, step: u32 },

    /// Настройка
    Adjusting,

    /// Завершено
    Done,

    /// Ошибка
    Error(String),
}

impl std::fmt::Display for ProgressStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgressStatus::Idle => write!(f, "Ожидание"),
            ProgressStatus::SearchingEdge { ch, step } => {
                write!(f, "Поиск края резонатора {}. Шаг: {}", ch + 1, step)
            }
            ProgressStatus::Adjusting => write!(f, "Настройка"),
            ProgressStatus::Done => write!(f, "Завершено"),
            ProgressStatus::Error(e) => write!(f, "Ошибка: {e}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RezInfo {
    id: usize,
    current_step: u32,
    current_freq: f32,
    state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressReport {
    pub status: ProgressStatus,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub measure_channel_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burn_channel_id: Option<u32>,

    pub rezonator_info: Vec<RezInfo>,
}

impl ProgressReport {
    pub fn new<'a>(
        progress: ProgressStatus,
        measure_channel_id: Option<u32>,
        burn_channel_id: Option<u32>,
        rezonator_info: Vec<RezInfo>,
    ) -> Self {
        Self {
            status: progress,
            measure_channel_id,
            burn_channel_id,
            rezonator_info,
        }
    }

    pub fn error<'a>(e: String, rezonator_info: Vec<RezInfo>) -> Self {
        Self {
            status: ProgressStatus::Error(e),
            measure_channel_id: None,
            burn_channel_id: None,
            rezonator_info,
        }
    }
}

impl Default for ProgressReport {
    fn default() -> Self {
        Self {
            status: ProgressStatus::Idle,
            measure_channel_id: None,
            burn_channel_id: None,
            rezonator_info: vec![],
        }
    }
}

pub struct AutoAdjustAllController {
    channel_count: usize,
    laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
    laser_setup_controller: Arc<Mutex<laser_precision_adjust::LaserSetupController>>,
    auto_adjust_limits: AutoAdjustLimits,
    update_interval: Duration,
    precision_ppm: f32,

    task: Option<tokio::task::JoinHandle<()>>,
    rx: Option<watch::Receiver<ProgressReport>>,
}

impl AutoAdjustAllController {
    pub fn new(
        channel_count: usize,
        laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
        laser_setup_controller: Arc<Mutex<laser_precision_adjust::LaserSetupController>>,
        auto_adjust_limits: AutoAdjustLimits,
        update_interval: Duration,
        precision_ppm: f32,
    ) -> Self {
        Self {
            channel_count,
            laser_controller,
            laser_setup_controller,
            auto_adjust_limits,
            update_interval,
            precision_ppm,

            task: None,
            rx: None,
        }
    }

    pub fn subscribe(&self) -> Option<watch::Receiver<ProgressReport>> {
        self.rx.as_ref().map(|rx| rx.clone())
    }

    pub fn get_status(&self) -> ProgressReport {
        self.rx
            .as_ref()
            .map(|rx| rx.borrow().clone())
            .unwrap_or(ProgressReport::default())
    }

    pub fn adjust(&mut self, target: f32) -> Result<(), Error> {
        if let Some(task) = &self.task {
            if !task.is_finished() {
                return Err(Error::AdjustInProgress);
            }
        }

        let channel_count = self.channel_count;
        let channels = (0..channel_count)
            .map(move |ch_id| ChannelRef::new(ch_id, channel_count))
            .collect::<Vec<_>>();

        let (tx, rx) = watch::channel(ProgressReport::default());

        self.rx.replace(rx);

        self.task.replace(tokio::spawn(adjust_task(
            tx,
            self.laser_controller.clone(),
            self.laser_setup_controller.clone(),
            self.auto_adjust_limits,
            self.update_interval,
            self.precision_ppm,
            channels.into_far_long_iterator(
                chrono::Duration::from_std(self.update_interval * 2).unwrap(),
            ),
            target,
        )));

        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), Error> {
        if let Some(task) = &self.task {
            if !task.is_finished() {
                task.abort();
                self.task = None;

                return Ok(());
            }
        }
        self.rx = None;
        Err(Error::NothingToCancel)
    }
}

async fn adjust_task(
    mut tx: watch::Sender<ProgressReport>,
    laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
    laser_setup_controller: Arc<Mutex<laser_precision_adjust::LaserSetupController>>,
    auto_adjust_limits: AutoAdjustLimits,
    update_interval: Duration,
    precision_ppm: f32,
    mut channel_iterator: FarLongIterator<ChannelRef>,
    target: f32,
) {
    const TRYS: usize = 10;

    if let Err(e) = find_edge(
        &mut tx,
        &laser_controller,
        &laser_setup_controller,
        auto_adjust_limits,
        update_interval,
        precision_ppm,
        &mut channel_iterator,
        target,
        TRYS,
    )
    .await
    {
        tx.send(ProgressReport::error(
            e.to_string(),
            gen_rez_info(channel_iterator.iter()),
        ))
        .ok();
    } else {
        tx.send(ProgressReport::new(
            ProgressStatus::Adjusting,
            None,
            None,
            gen_rez_info(channel_iterator.iter()),
        ))
        .ok();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn find_edge(
    tx: &mut watch::Sender<ProgressReport>,
    laser_controller: &Mutex<laser_precision_adjust::LaserController>,
    laser_setup_controller: &Mutex<laser_precision_adjust::LaserSetupController>,
    limits: AutoAdjustLimits,
    update_interval: Duration,
    precision_ppm: f32,
    channel_iterator: &mut FarLongIterator<ChannelRef>,
    target: f32,
    trys: usize,
) -> anyhow::Result<()> {
    let upper_limit = target * (1.0 + precision_ppm / 1_000_000.0);
    let lower_limit = target * (1.0 - precision_ppm / 1_000_000.0);

    for ch in 0..channel_iterator.len() as u32 {
        tx.send(ProgressReport::new(
            ProgressStatus::SearchingEdge { ch, step: 0 },
            Some(ch),
            None,
            gen_rez_info(channel_iterator.iter()),
        ))
        .ok();

        laser_controller
            .lock()
            .await
            .select_channel(ch, None, Some(trys))
            .await
            .map_err(|e| {
                HardwareLogickError(format!("Не удалось переключить канал лазера ({e:?})"))
            })?;

        let rx = {
            let mut guard = laser_setup_controller.lock().await;

            guard.select_channel(ch).await.map_err(|e| {
                HardwareLogickError(format!("Не удалось переключить частотомер ({e:?})"))
            })?;
            guard.subscribe()
        };

        let mut last_freq = match measure(
            rx.clone(),
            update_interval * 10,
            0.2,
            (upper_limit, target - limits.min_freq_offset),
            Some(Duration::from_millis(500)),
        )
        .await?
        {
            MeasureResult::Stable(f) => {
                // Частота стабильна и в диапазоне, можно искать край
                tracing::info!("Channel {} stable at {} Hz", ch, f);
                if f < upper_limit && f > lower_limit {
                    // сразу годный, стоп!
                    channel_iterator.get_mut(ch as usize).unwrap().update_state(
                        ChannelState::Ok,
                        f,
                        0,
                    );
                    continue;
                } else {
                    channel_iterator.get_mut(ch as usize).unwrap().update_state(
                        ChannelState::InProcess,
                        f,
                        0,
                    );
                }
                f
            }
            MeasureResult::Unstable(boxplot) => {
                // Частота нестабильна - брак
                channel_iterator.get_mut(ch as usize).unwrap().update_state(
                    ChannelState::Unsatable,
                    boxplot.median(),
                    0,
                );
                continue;
            }
            MeasureResult::OutOfRange(f) => {
                // Частота вне диапазона - брак
                channel_iterator.get_mut(ch as usize).unwrap().update_state(
                    ChannelState::OutOfRange,
                    f,
                    0,
                );
                continue;
            }
        };

        loop {
            let current_step = {
                let mut guard = laser_controller.lock().await;
                let step = guard.get_current_step();
                tx.send(ProgressReport::new(
                    ProgressStatus::SearchingEdge { ch, step },
                    Some(ch),
                    Some(ch),
                    gen_rez_info(channel_iterator.iter()),
                ))
                .ok();
                match guard
                    .burn(1, Some(limits.edge_detect_interval as i32), Some(trys))
                    .await
                {
                    Ok(()) => { /* nothing */ }
                    Err(laser_precision_adjust::Error::Laser(e)) => {
                        if e.kind() == std::io::ErrorKind::InvalidInput {
                            // Достигнут предел перемещений
                            channel_iterator.get_mut(ch as usize).unwrap().update_state(
                                ChannelState::NoReaction,
                                last_freq,
                                guard.get_current_step(),
                            );
                            break;
                        } else {
                            Err(HardwareLogickError(format!(
                                "Не удалось сделать шаг ({e:?})"
                            )))?;
                        }
                    }
                    Err(e) => Err(HardwareLogickError(format!(
                        "Не удалось сделать шаг ({e:?})"
                    )))?,
                }
                guard.get_current_step()
            };

            match measure(
                rx.clone(),
                update_interval * 5,
                0.2,
                (upper_limit, 0.0),
                None,
            )
            .await?
            {
                MeasureResult::Stable(f) => {
                    // нет реакции
                    if f < upper_limit && f > lower_limit {
                        // годный, стоп!
                        channel_iterator.get_mut(ch as usize).unwrap().update_state(
                            ChannelState::Ok,
                            f,
                            current_step,
                        );
                        break;
                    } else {
                        channel_iterator.get_mut(ch as usize).unwrap().update_state(
                            ChannelState::InProcess,
                            f,
                            current_step,
                        );
                        last_freq = f;
                        continue;
                    }
                }
                MeasureResult::Unstable(bp) => {
                    channel_iterator.get_mut(ch as usize).unwrap().update_state(
                        ChannelState::Ok,
                        bp.upper_bound(),
                        current_step,
                    );
                    break;
                }
                MeasureResult::OutOfRange(f) => {
                    channel_iterator.get_mut(ch as usize).unwrap().update_state(
                        ChannelState::OutOfRange,
                        f,
                        current_step,
                    );
                    break; // Брак
                }
            }
        }

        channel_iterator.get_mut(ch as usize).unwrap().touch();
    }

    Ok(())
}

enum MeasureResult {
    Stable(f32),
    Unstable(BoxPlot<f32>),
    OutOfRange(f32),
}

async fn measure(
    mut m: watch::Receiver<laser_precision_adjust::LaserSetupStatus>,
    timeout: Duration,
    stable_range: f32,
    work_range: (f32, f32),
    wait_before: Option<Duration>,
) -> Result<MeasureResult, watch::error::RecvError> {
    let mut data = vec![];

    if let Some(wait_before) = wait_before {
        tokio::time::sleep(wait_before).await;
    }

    let start = std::time::Instant::now();
    while std::time::Instant::now() - start < timeout {
        m.changed().await?;
        let status = m.borrow();
        data.push(status.current_frequency);
    }

    let boxplot = BoxPlot::new(&data);
    Ok(if boxplot.iqr() < stable_range {
        if boxplot.median() > work_range.0 || boxplot.median() < work_range.1 {
            MeasureResult::OutOfRange(boxplot.median())
        } else {
            MeasureResult::Stable(boxplot.median())
        }
    } else {
        MeasureResult::Unstable(boxplot)
    })
}

fn gen_rez_info<'a>(iter: impl Iterator<Item = &'a ChannelRef>) -> Vec<RezInfo> {
    iter.map(|r| RezInfo {
        id: r.id,
        current_freq: r.current_freq,
        current_step: r.current_step,
        state: r.get_state().to_status_icon(),
    })
    .collect()
}
