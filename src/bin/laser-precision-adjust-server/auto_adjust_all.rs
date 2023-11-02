use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Local};
use laser_precision_adjust::{
    box_plot::BoxPlot, AutoAdjustLimits, ForecastConfig, PrivStatusEvent,
};
use serde::Serialize;
use tokio::sync::{watch, Mutex};

use crate::far_long_iterator::{FarLongIterator, FarLongIteratorItem, IntoFarLongIterator};

const MEASRE_COUNT_NORMAL: u32 = 3;

enum MeasureResult {
    Stable(f32),
    Unstable(BoxPlot<f32>),
    OutOfRange(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChannelState {
    UnknownInit,
    Adjustig,
    Unsatable,
    OutOfRange,
    Limit,
    Verify,
    Ok,
}

enum BurnEvent {
    ErrorBan(u32),
    Done(u32, u32),
}

impl ChannelState {
    fn to_status_icon(&self) -> String {
        match self {
            ChannelState::UnknownInit => "Неизвестно",
            ChannelState::Adjustig => "Настройка",
            ChannelState::Unsatable => "Сломан или нестабилен",
            ChannelState::OutOfRange => "Вне диапазона настройки",
            ChannelState::Limit => "Превышен лимит шагов",
            ChannelState::Verify => "Проверка",
            ChannelState::Ok => "Настроен",
        }
        .to_owned()
    }
}

#[derive(Clone)]
pub struct ChannelRef {
    id: usize,
    last_touched: DateTime<Local>,
    total_channels: usize,
    state: ChannelState,
    initial_freq: Option<f32>,
    current_freq: f32,
    current_step: u32,
}

impl ChannelRef {
    pub fn new(id: usize, total_channels: usize) -> Self {
        Self {
            id,
            last_touched: Local::now(),
            total_channels,
            state: ChannelState::UnknownInit,
            initial_freq: None,
            current_freq: 0.0,
            current_step: 0,
        }
    }

    fn touch(&mut self) {
        self.last_touched = Local::now();
    }

    fn update_state(&mut self, state: ChannelState, freq: f32, step: u32, ok: bool) {
        tracing::debug!(
            "Rezonator {} update state {:?}: F={}, step={}",
            self.id,
            state,
            freq,
            step
        );

        if self.initial_freq.is_none() && ok {
            self.initial_freq.replace(freq);
        }

        self.state = if self.state == ChannelState::Verify && state == ChannelState::Verify {
            ChannelState::Ok
        } else {
            state
        };

        self.current_freq = freq;
        self.current_step = step;
    }

    fn ban(&mut self, state: ChannelState) {
        tracing::debug!("Rezonator {} banned: {:?}", self.id, state);
        self.state = state;
    }

    fn get_state(&self) -> ChannelState {
        self.state
    }

    fn current_step(&self) -> u32 {
        self.current_step
    }

    fn set_step(&mut self, step: u32) {
        self.current_step = step;
    }

    fn age_found(&self) -> bool {
        if let Some(initial_freq) = self.initial_freq {
            self.current_freq - initial_freq > 0.5
        } else {
            false
        }
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

    fn last_touched(&self) -> DateTime<Local> {
        self.last_touched
    }

    fn is_valid(&self) -> bool {
        match self.state {
            ChannelState::UnknownInit | ChannelState::Verify | ChannelState::Adjustig => true,
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
    initial_freq: f32,
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
    precision_adjust: Arc<Mutex<laser_precision_adjust::PrecisionAdjust2>>,
    auto_adjust_limits: AutoAdjustLimits,
    update_interval: Duration,
    precision_ppm: f32,
    forecast_config: ForecastConfig,
    fast_forward_step_limit: u32,

    task: Option<tokio::task::JoinHandle<()>>,
    rx: Option<watch::Receiver<ProgressReport>>,
}

impl AutoAdjustAllController {
    pub fn new(
        channel_count: usize,
        laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
        laser_setup_controller: Arc<Mutex<laser_precision_adjust::LaserSetupController>>,
        precision_adjust: Arc<Mutex<laser_precision_adjust::PrecisionAdjust2>>,
        auto_adjust_limits: AutoAdjustLimits,
        update_interval: Duration,
        precision_ppm: f32,
        forecast_config: ForecastConfig,
        fast_forward_step_limit: u32,
    ) -> Self {
        Self {
            channel_count,
            laser_controller,
            laser_setup_controller,
            precision_adjust,
            auto_adjust_limits,
            update_interval,
            precision_ppm,
            forecast_config,
            fast_forward_step_limit,

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
            self.forecast_config,
            self.fast_forward_step_limit,
            self.precision_adjust.clone(),
        )));

        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), Error> {
        if let Some(task) = &self.task {
            if !task.is_finished() {
                task.abort();
                self.task = None;
                self.rx = None;

                return Ok(());
            }
        }
        self.rx = None;
        Err(Error::NothingToCancel)
    }
}

#[derive(Clone, Copy, Debug)]
enum Trys<const TRYS: u32> {
    Ok,
    Unstable(u32),
    OutOfrange(u32),
}

impl<const TRYS: u32> Trys<TRYS> {
    pub fn mark_ok(&mut self) {
        *self = Self::Ok;
    }

    pub fn more_trys_avalable(&self) -> bool {
        match self {
            Trys::Ok => true,
            Trys::Unstable(n) => *n > 0,
            Trys::OutOfrange(n) => *n > 0,
        }
    }

    pub fn mark_unstable(&mut self) {
        match self {
            Trys::Unstable(n) => *self = Self::Unstable(n.saturating_sub(1)),
            _ => *self = Self::Unstable(TRYS / 2),
        }
    }

    pub fn mark_out_of_range(&mut self) {
        match self {
            Trys::OutOfrange(n) => *self = Self::OutOfrange(n.saturating_sub(1)),
            _ => *self = Self::OutOfrange(2),
        }
    }
}

impl<const TRYS: u32> Default for Trys<TRYS> {
    fn default() -> Self {
        Trys::Unstable(TRYS)
    }
}

async fn adjust_task(
    tx: watch::Sender<ProgressReport>,
    laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
    laser_setup_controller: Arc<Mutex<laser_precision_adjust::LaserSetupController>>,
    auto_adjust_limits: AutoAdjustLimits,
    update_interval: Duration,
    precision_ppm: f32,
    mut channel_iterator: FarLongIterator<ChannelRef>,
    target: f32,
    forecast_config: ForecastConfig,
    fast_forward_step_limit: u32,
    precision_adjust: Arc<Mutex<laser_precision_adjust::PrecisionAdjust2>>,
) {
    const TRYS: u32 = 5;

    let switch_channel_wait = Duration::from_millis(laser_precision_adjust::SWITCH_CHANNEL_WAIT_MS);

    let upper_limit = target * (1.0 + precision_ppm / 1_000_000.0);
    let lower_limit = target * (1.0 - precision_ppm / 1_000_000.0);
    let absolute_low_limit = target - auto_adjust_limits.min_freq_offset;
    let mut prev_ch = None;

    let mut trys_counters = vec![Trys::<TRYS>::default(); channel_iterator.len()];

    let rx = laser_setup_controller.lock().await.subscribe();

    let mut extend_stable_time = false;

    let (burn_tx, mut burn_rx) = tokio::sync::mpsc::channel(1);

    while let Some(ch_id) = channel_iterator.next() {
        let burn_ch = match burn_rx.try_recv() {
            Ok(BurnEvent::ErrorBan(ch)) => {
                channel_iterator
                    .get_mut(ch as usize)
                    .unwrap()
                    .ban(ChannelState::Limit);
                Some(ch)
            }
            Ok(BurnEvent::Done(ch, step)) => {
                channel_iterator
                    .get_mut(ch as usize)
                    .unwrap()
                    .set_step(step);
                Some(ch)
            }
            Err(_) => None,
        };

        let mut wait_interval = Duration::ZERO;
        let channel_switched = if Some(ch_id) != prev_ch {
            let mut guard = laser_setup_controller.lock().await;
            if let Err(e) = guard.select_channel(ch_id as u32).await {
                tracing::error!("Failed to switch freqmeter channel: {e:?}");
                continue;
            }
            guard.delay(switch_channel_wait).await;

            precision_adjust
                .lock()
                .await
                .push_event(PrivStatusEvent {
                    chanel_select: Some(ch_id as u32),
                    ..Default::default()
                })
                .await;

            wait_interval += switch_channel_wait;
            prev_ch = Some(ch_id);

            true
        } else {
            false
        };

        let rez_info = gen_rez_info(channel_iterator.iter());
        let ch = channel_iterator.get_mut(ch_id).unwrap();
        {
            let now = Local::now();
            let after_last_touch = now - ch.last_touched();
            let min_touch_wait = chrono::Duration::from_std(Duration::from_secs_f32(3.5)).unwrap();
            wait_interval = if after_last_touch < min_touch_wait {
                wait_interval.max((min_touch_wait - after_last_touch).to_std().unwrap())
            } else {
                wait_interval
            };
        }

        let step = ch.current_step();

        tx.send(ProgressReport::new(
            ProgressStatus::Adjusting,
            Some(ch_id as u32),
            burn_ch,
            rez_info,
        ))
        .ok();

        let current_freq = match measure(
            rx.clone(),
            update_interval * MEASRE_COUNT_NORMAL,
            0.2,
            (upper_limit, absolute_low_limit),
            if extend_stable_time {
                extend_stable_time = false;
                Some(wait_interval * 3)
            } else {
                Some(wait_interval)
            },
            1,
        )
        .await
        {
            Ok(MeasureResult::Stable(f)) => {
                // успешно
                trys_counters[ch_id].mark_ok();
                if f > target || f + forecast_config.median_freq_grow > target {
                    // stop
                    tracing::warn!("Ch {} verify: f={}", ch_id, f);
                    ch.update_state(ChannelState::Verify, f, step, true);
                    ch.touch();
                    continue;
                } else {
                    // continue
                    ch.update_state(ChannelState::Adjustig, f, step, true);
                    f
                }
            }
            Ok(MeasureResult::Unstable(bxplt)) => {
                // Частота нестабильна, возможно резонатор не остыли или сломан
                extend_stable_time = true;

                if channel_switched && (bxplt.median() - target).abs() > 1000.0 {
                    // похоже, что канал нерабочий
                    trys_counters[ch_id].mark_out_of_range();
                } else {
                    trys_counters[ch_id].mark_unstable();
                };

                tracing::warn!(
                    "Ch {} unsatble: {:?}, remaning={:?}",
                    ch_id,
                    bxplt,
                    trys_counters[ch_id]
                );

                if trys_counters[ch_id].more_trys_avalable() {
                    ch.update_state(ChannelState::Adjustig, bxplt.q3(), step, false);
                    ch.touch();
                } else {
                    ch.ban(ChannelState::Unsatable);
                }
                continue;
            }
            Ok(MeasureResult::OutOfRange(f)) => {
                // Вышел за прелелы диопазона, но стабилен
                trys_counters[ch_id].mark_out_of_range();
                tracing::warn!(
                    "Ch {} out of range {}, remaning={:?}",
                    ch_id,
                    f,
                    trys_counters[ch_id]
                );
                if trys_counters[ch_id].more_trys_avalable() {
                    ch.update_state(ChannelState::Adjustig, f, step, false);
                    ch.touch();
                    continue;
                } else {
                    ch.update_state(ChannelState::OutOfRange, f, step, true);
                    continue;
                }
            }
            Err(e) => {
                tx.send(ProgressReport::error(
                    e.to_string(),
                    gen_rez_info(channel_iterator.iter()),
                ))
                .ok();
                tracing::error!("Failed to measure channel {ch_id}: {e:?}");
                trys_counters[ch_id].mark_unstable();
                continue;
            }
        };

        // Ударяем
        {
            let soft_mode = current_freq > lower_limit;
            let steps_to_burn = if soft_mode {
                // soft touch
                tracing::warn!(
                    "f:{} > Lower_limit:{} - soft mode",
                    current_freq,
                    lower_limit
                );
                1
            } else if !ch.age_found() {
                auto_adjust_limits.edge_detect_interval
            } else {
                fast_forward_step_limit
                    .min(((target - current_freq) / forecast_config.max_freq_grow).ceil() as u32)
            };

            ch.touch();
            precision_adjust
                .lock()
                .await
                .push_event(PrivStatusEvent {
                    shot_mark: Some(true),
                    step: Some(steps_to_burn as i32),
                    ..Default::default()
                })
                .await;

            tokio::spawn(burn_task(
                laser_controller.clone(),
                steps_to_burn,
                ch_id as u32,
                Some(step),
                Some(TRYS as usize),
                soft_mode,
                burn_tx.clone(),
            ));
        }
    }

    // Готово
    tx.send(ProgressReport::new(
        ProgressStatus::Done,
        None,
        None,
        gen_rez_info(channel_iterator.iter()),
    ))
    .ok();
}

async fn measure(
    mut m: watch::Receiver<laser_precision_adjust::LaserSetupStatus>,
    timeout: Duration,
    stable_range: f32,
    work_range: (f32, f32),
    wait_before: Option<Duration>,
    trys: usize,
) -> Result<MeasureResult, watch::error::RecvError> {
    let mut data = vec![];

    if let Some(wait_before) = wait_before {
        tokio::time::sleep(wait_before).await;
    }

    for i in 0..trys {
        let start = std::time::Instant::now();
        while std::time::Instant::now() - start < timeout {
            m.changed().await?;
            let status = m.borrow();
            tracing::trace!("measure(): F={}", status.current_frequency);
            data.push(status.current_frequency);
        }

        let boxplot = BoxPlot::new(&data);
        if boxplot.iqr() < stable_range {
            return if boxplot.median() > work_range.0 || boxplot.median() < work_range.1 {
                Ok(MeasureResult::OutOfRange(boxplot.median()))
            } else {
                Ok(MeasureResult::Stable(boxplot.median()))
            };
        } else if i == trys - 1 {
            return Ok(MeasureResult::Unstable(boxplot));
        }
    }

    panic!("trys == 0")
}

fn gen_rez_info<'a>(iter: impl Iterator<Item = &'a ChannelRef>) -> Vec<RezInfo> {
    iter.map(|r| RezInfo {
        id: r.id,
        initial_freq: r.initial_freq.unwrap_or(0.0),
        current_freq: r.current_freq,
        current_step: r.current_step,
        state: {
            let state = r.get_state();
            if state != ChannelState::Adjustig || r.age_found() {
                state.to_status_icon()
            } else {
                "Поиск края".to_owned()
            }
        },
    })
    .collect()
}

async fn burn_task(
    laser_controller: Arc<Mutex<laser_precision_adjust::LaserController>>,
    burn_count: u32,
    channel: u32,
    initial_step: Option<u32>,
    trys: Option<usize>,
    soft_mode: bool,
    burn_tx: tokio::sync::mpsc::Sender<BurnEvent>,
) {
    let mut guard = laser_controller.lock().await;

    let post_result = move |res| {
        match res {
            Ok((ch, step)) => {
                burn_tx.try_send(BurnEvent::Done(ch, step)).ok();
            }
            Err((ch, e)) => {
                // ban this channel to prevent infinty loop
                tracing::error!("Failed to burn prev step: {:?}", e);
                burn_tx.try_send(BurnEvent::ErrorBan(ch)).ok();
            }
        }
    };

    if let Err(e) = guard.select_channel(channel, initial_step, trys).await {
        post_result(Err((channel, e)));
        return;
    }
    match guard.burn(burn_count, Some(1), trys, soft_mode).await {
        Ok(_) => post_result(Ok((channel, guard.get_current_step()))),
        Err(e) => post_result(Err((channel, e))),
    }
}
