use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Local};
use laser_precision_adjust::box_plot::BoxPlot;
use tokio::sync::Mutex;

use crate::{
    auto_adjust_single_controller::HardwareLogickError,
    far_long_iterator::{FarLongIterator, FarLongIteratorItem, IntoFarLongIterator},
};

#[derive(Clone)]
pub struct ChannelRef {
    id: usize,
    last_selected: DateTime<Local>,
    total_channels: usize,
}

impl ChannelRef {
    pub fn new(id: usize, total_channels: usize) -> Self {
        Self {
            id,
            last_selected: Local::now(),
            total_channels,
        }
    }

    fn select(&mut self) {
        self.last_selected = Local::now();
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
        true /* TODO */
    }
}

pub enum Error {
    AdjustInProgress,
    NothingToCancel,
}

enum State {
    /// Бездействие
    Idle,

    /// Поиск края
    SearchingEdge,

    /// Настройка
    Adjusting,

    /// Завершено
    Done,

    /// Ошибка
    Error(String),
}

pub struct ProgressReport {
    state: State,
}

pub struct AutoAdjustAllController {
    channel_count: usize,
    laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
    laser_setup_controller: Arc<Mutex<laser_precision_adjust::LaserSetupController>>,
    auto_adjust_limits: laser_precision_adjust::AutoAdjustLimits,
    update_interval: Duration,
    precision_ppm: f32,

    task: Option<tokio::task::JoinHandle<()>>,
}

impl AutoAdjustAllController {
    pub fn new(
        channel_count: usize,
        laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
        laser_setup_controller: Arc<Mutex<laser_precision_adjust::LaserSetupController>>,
        auto_adjust_limits: laser_precision_adjust::AutoAdjustLimits,
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
        }
    }

    pub fn adjust(&mut self) -> Result<tokio::sync::watch::Receiver<ProgressReport>, Error> {
        if let Some(task) = &self.task {
            if !task.is_finished() {
                return Err(Error::AdjustInProgress);
            }
        }

        let channel_count = self.channel_count;
        let channels = (0..channel_count)
            .map(move |ch_id| ChannelRef::new(ch_id, channel_count))
            .collect::<Vec<_>>();

        let (tx, rx) = tokio::sync::watch::channel(ProgressReport { state: State::Idle });

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
        )));

        Ok(rx)
    }

    pub fn cancel(&mut self) -> Result<(), Error> {
        if let Some(task) = &self.task {
            if !task.is_finished() {
                task.abort();
                self.task = None;

                return Ok(());
            }
        }
        Err(Error::NothingToCancel)
    }
}

async fn adjust_task(
    mut tx: tokio::sync::watch::Sender<ProgressReport>,
    laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
    laser_setup_controller: Arc<Mutex<laser_precision_adjust::LaserSetupController>>,
    auto_adjust_limits: laser_precision_adjust::AutoAdjustLimits,
    update_interval: Duration,
    precision_ppm: f32,
    mut channel_iterator: FarLongIterator<ChannelRef>,
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
        TRYS,
    )
    .await
    {
        tx.send(ProgressReport {
            state: State::Error(e.to_string()),
        })
        .ok();
    } else {
        tx.send(ProgressReport {
            state: State::Adjusting,
        })
        .ok();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn find_edge(
    tx: &mut tokio::sync::watch::Sender<ProgressReport>,
    laser_controller: &Mutex<laser_precision_adjust::LaserController>,
    laser_setup_controller: &Mutex<laser_precision_adjust::LaserSetupController>,
    auto_adjust_limits: laser_precision_adjust::AutoAdjustLimits,
    update_interval: Duration,
    precision_ppm: f32,
    channel_iterator: &mut FarLongIterator<ChannelRef>,
    trys: usize,
) -> anyhow::Result<()> {
    tx.send(ProgressReport {
        state: State::SearchingEdge,
    })
    .ok();
    for ch in 0..channel_iterator.count() as u32 {
        laser_controller
            .lock()
            .await
            .select_channel(ch, None, Some(trys))
            .await
            .map_err(|e| {
                HardwareLogickError(format!("Не удалось переключить канал лазера ({e:?})"))
            })?;

        let mut guard = laser_setup_controller.lock().await;

        guard.select_channel(ch).await.map_err(|e| {
            HardwareLogickError(format!("Не удалось переключить частотомера ({e:?})"))
        })?;

        match measure(guard.subscribe(), Duration::from_secs(1)) {
            MeasureResult::Stable(f) => {}
            MeasureResult::Unstable(boxplot) => {}
            MeasureResult::OutOfRange(f) => {}
        }
    }

    Ok(())
}

enum MeasureResult {
    Stable(f32),
    Unstable(BoxPlot<f32>),
    OutOfRange(f32),
}

fn measure(
    m: tokio::sync::watch::Receiver<laser_precision_adjust::LaserSetupStatus>,
    timeout: Duration,
) -> MeasureResult {
    MeasureResult::OutOfRange(0.0)
}
