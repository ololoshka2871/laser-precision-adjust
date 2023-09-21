use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{mpsc::error::SendError, Mutex},
    task::JoinHandle,
    time,
};

use laser_precision_adjust::{AutoAdjustLimits, PrecisionAdjust};

use crate::predict::{Fragment, Predictor};

#[derive(PartialEq, Clone, Copy)]
pub enum State {
    // Бездействие
    Idle,

    // Поиск края
    DetctingEdge,

    // "Грубая" настройка
    // Разница между текущим значением частоты и целью делится на прогноз самого сильно измненеия
    // Полученое число округляется в меньшую сторону, но не менее 1
    // Делается указанное количекство проходов
    DihotomyStepping,

    // "Точный" шаг
    // Если наиболеьшее значение прогноза выше минимально-необходимой частоты, то текущая частота меньше минимально-необходимой
    // делается 1 шаг и ожидается полное охладдение
    PrecisionStepping,

    // Пока текущая частота выше минимально-необходимой частоты, но ниже целевой, а верхний предел прогноза ниже верхнего допустимого предела
    // Делается 1 шаг со смещщением -1, ног не более <max_reves_steps> шагов
    ReverseStepping,
}

pub enum AutoAdjustStateReport {
    Progress(String),
    Error(String),
    Finished,
}

pub struct AutoAdjestController {
    config: AutoAdjustLimits,
    update_interval_ms: u32,
    state: Arc<Mutex<State>>,
    task: Option<JoinHandle<Result<(), SendError<AutoAdjustStateReport>>>>,
}

impl AutoAdjestController {
    pub fn new(config: AutoAdjustLimits, update_interval_ms: u32) -> Self {
        Self {
            config,
            update_interval_ms,
            state: Arc::new(Mutex::new(State::Idle)),
            task: None,
        }
    }

    pub async fn try_start(
        &mut self,
        channel: u32,
        predictor: Arc<Mutex<Predictor<f64>>>,
        precision_adjust: Arc<Mutex<PrecisionAdjust>>,
    ) -> Result<tokio::sync::mpsc::Receiver<AutoAdjustStateReport>, &'static str> {
        if *self.state.lock().await == State::Idle {
            let (tx, rx) = tokio::sync::mpsc::channel(1);

            tracing::warn!("Start auto-adjustion channel {}", channel);
            self.task.replace(tokio::spawn(adjust_task(
                channel,
                self.update_interval_ms,
                tx,
                self.state.clone(),
                predictor,
                precision_adjust,
                self.config,
            )));

            Ok(rx)
        } else {
            Err("Busy!")
        }
    }

    pub async fn cancel(&mut self) -> Result<(), &'static str> {
        if *self.state.lock().await == State::Idle {
            Err("Not running")
        } else if let Some(task) = &self.task {
            if !task.is_finished() {
                tracing::warn!("Abort auto-adjust");
                task.abort();
                time::sleep(Duration::from_secs(1)).await;

                // Leave in safe state
                *self.state.lock().await = State::Idle;

                Ok(())
            } else {
                *self.state.lock().await = State::Idle;
                Err("Already finished")
            }
        } else {
            Err("Unknown state")
        }
    }

    pub async fn current_state(&self) -> State {
        *self.state.lock().await
    }
}

//-----------------------------------------------------------------------------

async fn adjust_task(
    channel: u32,
    update_interval_ms: u32,
    status_report_q: tokio::sync::mpsc::Sender<AutoAdjustStateReport>,
    state: Arc<Mutex<State>>,
    predictor: Arc<Mutex<Predictor<f64>>>,
    precision_adjust: Arc<Mutex<PrecisionAdjust>>,
    config: AutoAdjustLimits,
) -> Result<(), SendError<AutoAdjustStateReport>> {
    // Стадия 1: поиск края
    *state.lock().await = State::DetctingEdge;
    status_report_q
        .send(AutoAdjustStateReport::Progress("Detecting edge".into()))
        .await?;

    match find_edge(
        channel,
        update_interval_ms,
        &predictor,
        &precision_adjust,
        config.edge_detect_interval,
    )
    .await
    {
        Ok(edge_pos) => {
            tracing::warn!("Edge found at step {}", edge_pos);
            //*state.lock().await = State::DihotomyStepping;
        }
        Err(e) => {
            tracing::error!("Edge not found: {}", e);
            *state.lock().await = State::Idle;
            return Ok(());
        }
    }

    // success
    *state.lock().await = State::Idle;
    Ok(())
}

async fn burn(precision_adjust: &Mutex<PrecisionAdjust>) -> Result<(), String> {
    precision_adjust
        .lock()
        .await
        .burn()
        .await
        .map_err(|e| format!("Не удалось включить лазер ({e:?})"))
}

async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

async fn get_last_fragment(
    predictor: &Mutex<Predictor<f64>>,
    channel: u32,
) -> Option<Fragment<f64>> {
    predictor.lock().await.get_last_fragment(channel).await
}

async fn find_edge(
    channel: u32,
    update_interval_ms: u32,
    predictor: &Mutex<Predictor<f64>>,
    precision_adjust: &Mutex<PrecisionAdjust>,
    edge_detect_interval: u32,
) -> Result<u32, String> {
    use std::cmp::min;

    // switch channel
    precision_adjust
        .lock()
        .await
        .select_channel(channel)
        .await
        .map_err(|e| format!("Не удалось переключить канал ({e:?})"))?;

    // switch delay
    sleep_ms(min((update_interval_ms * 5) as u64, 500)).await;

    let mut current_step = 0;

    let mut last_fragment_ts = get_last_fragment(predictor, channel)
        .await
        .map(|f| f.start_timestamp());
    loop {
        // Прожиг
        burn(precision_adjust).await?;

        // Ждем пока не появится новый фрагмент
        let mut last_fragment: Option<Fragment<f64>> = None;
        for _ in 0..10 {
            sleep_ms(100).await;
            last_fragment = get_last_fragment(predictor, channel).await;
            if let Some(last_fragment) = &last_fragment {
                if Some(last_fragment.start_timestamp()) != last_fragment_ts {
                    // новый фрагмент появился
                    last_fragment_ts.replace(last_fragment.start_timestamp());
                    break;
                }
            }
        }

        // посик во фрагменте повышения частоты не менее чем на 0.2 Гц
        if let Some(last_fragment) = &last_fragment {
            let box_plot = last_fragment.box_plot();
            if box_plot.q3() - box_plot.q1() >= 0.2 {
                // нашли
                return Ok(current_step);
            }
        }

        // не найдено, шагаем на edge_detect_interval
        match precision_adjust
            .lock()
            .await
            .step(edge_detect_interval as i32)
            .await
        {
            Ok(_) => {
                current_step += edge_detect_interval;
            } // ok
            Err(laser_precision_adjust::Error::Logick(_)) => break, // конец хода
            Err(e) => return Err(format!("Не удалось сделать шаг ({e:?})")),
        }
    }

    Err(format!(
        "Край не найден, достигнут лимит перемещения ({current_step})"
    ))
}
