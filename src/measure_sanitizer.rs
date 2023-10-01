use std::time::Duration;
use tokio::sync::watch::{Receiver, Sender};

use crate::{box_plot::BoxPlot, Status};

#[derive(Clone, Copy, Debug)]
pub struct CoollingDownInfo {
    pub forecast: f32,
    pub current: f32,
}

#[derive(Copy, Clone, Debug)]
pub enum MeasureSanitizerState {
    Waiting,
    Unstable(Duration, u32, BoxPlot<f32>),
    CoollingDown(Duration, u32, CoollingDownInfo),
    Stable(Duration, u32, f32),
}

const SANITIZER_LEN: usize = 5;

pub struct MeasureSanitizer {
    state: Receiver<MeasureSanitizerState>,
}

impl MeasureSanitizer {
    pub fn new(status_rx: Receiver<Status>, stable_val: f32) -> Self {
        let (tx, rx) = tokio::sync::watch::channel(MeasureSanitizerState::Waiting);
        tokio::spawn(watcher_task(tx, status_rx, stable_val));
        Self { state: rx }
    }

    pub fn capture_current(&self) -> MeasureSanitizerState {
        *self.state.borrow()
    }

    pub async fn try_get_correct(
        &self,
        timeout: Duration,
    ) -> Result<(Duration, u32, f32), MeasureSanitizerState> {
        // await while self.state is stable and return it or return current state if timeout
        let mut rx = self.state.clone();
        loop {
            match tokio::time::timeout(timeout, rx.changed()).await {
                Ok(Ok(_)) => {
                    // changed
                    if let MeasureSanitizerState::Stable(ts, ch, freq) = *self.state.borrow() {
                        return Ok((ts, ch, freq));
                    }
                }
                _ => {
                    // timeout
                    return Err(*self.state.borrow());
                }
            }
        }
    }
}

async fn watcher_task(
    tx: Sender<MeasureSanitizerState>,
    mut status_rx: Receiver<Status>,
    stable_val: f32,
) {
    let mut last_channel = None;
    let mut data = vec![];
    loop {
        if let Err(_) = status_rx.changed().await {
            break;
        }

        let status = status_rx.borrow().clone();

        if status.shot_mark || Some(status.current_channel) != last_channel {
            // channel changed or shot mark, reset sanitizer
            last_channel.replace(status.current_channel);
            data = vec![status.current_frequency];
            tx.send(MeasureSanitizerState::Waiting).unwrap();
        } else {
            data.push(status.current_frequency);
            if data.len() > SANITIZER_LEN {
                data.remove(0);
            }

            let box_plot = BoxPlot::new(&data);
            if box_plot.iqr() > stable_val {
                if let Some(cd_info) = detect_raising(&data) {
                    tx.send(MeasureSanitizerState::CoollingDown(
                        status.since_start,
                        status.current_channel,
                        cd_info,
                    ))
                    .unwrap();
                } else {
                    tx.send(MeasureSanitizerState::Unstable(
                        status.since_start,
                        status.current_channel,
                        box_plot,
                    ))
                    .unwrap();
                }
            } else {
                tx.send(MeasureSanitizerState::Stable(
                    status.since_start,
                    status.current_channel,
                    box_plot.median(),
                ))
                .unwrap();
            }
        }
    }
}

fn detect_raising(data: &[f32]) -> Option<CoollingDownInfo> {
    if let Some((min_index, t_zero)) = crate::predict::find_min(data) {
        let x = data[min_index..]
            .iter()
            .map(move |t| (*t - t_zero) / crate::predict::NORMAL_T as f32)
            .collect::<Vec<_>>();
        let min_f = data[min_index];
        let y = data[min_index..]
            .iter()
            .map(move |f| *f - min_f)
            .collect::<Vec<_>>();
        if let Ok((a, _)) = crate::predict::aproximate_exp(x, &y) {
            let forecast = a + min_f;
            let current = data.last().copied().unwrap_or_default();
            return Some(CoollingDownInfo { forecast, current });
        }
    }
    None
}
