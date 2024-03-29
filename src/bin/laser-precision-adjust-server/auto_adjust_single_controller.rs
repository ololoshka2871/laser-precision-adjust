use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{mpsc, Mutex},
    task::JoinHandle,
    time,
};

use laser_precision_adjust::{
    box_plot::BoxPlot,
    predict::{Fragment, Predictor},
    AutoAdjustLimits, PrecisionAdjust2, AdjustConfig,
};

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
    // Пока наибольшее значение прогноза ниже целевой частоты делается 1 шаг и ожидается полное охладдение
    PrecisionStepping,

    // Конец
    End,
}

#[derive(Clone, Debug)]
pub struct HardwareLogickError(pub String);

impl std::fmt::Display for HardwareLogickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HardwareLogickError {}

pub enum AutoAdjustSingleStateReport {
    Progress(String),
    Error(String),
    Finished(String),
}

pub struct AutoAdjustSingleController {
    config: AutoAdjustLimits,
    update_interval_ms: u32,
    freqmeter_config: Arc<Mutex<AdjustConfig>>,
    state: Arc<Mutex<State>>,
    task: Option<JoinHandle<Result<(), anyhow::Error>>>,
}

impl AutoAdjustSingleController {
    pub fn new(config: AutoAdjustLimits, update_interval_ms: u32, freqmeter_config: Arc<Mutex<AdjustConfig>>) -> Self {
        Self {
            config,
            update_interval_ms,
            freqmeter_config,
            state: Arc::new(Mutex::new(State::Idle)),
            task: None,
        }
    }

    pub async fn try_start(
        &mut self,
        channel: u32,
        predictor: Arc<Mutex<Predictor<f64>>>,
        precision_adjust: Arc<Mutex<PrecisionAdjust2>>,
        traget_frequency: f32,
    ) -> Result<mpsc::Receiver<AutoAdjustSingleStateReport>, &'static str> {
        if *self.state.lock().await == State::Idle {
            let (tx, rx) = mpsc::channel(1);

            tracing::warn!("Start auto-adjustion channel {}", channel);
            self.task.replace(tokio::spawn(adjust_task(
                channel,
                self.update_interval_ms,
                tx,
                self.state.clone(),
                predictor,
                precision_adjust,
                self.config,
                traget_frequency as f64,
                self.freqmeter_config.clone(),
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
    status_report_q: mpsc::Sender<AutoAdjustSingleStateReport>,
    state: Arc<Mutex<State>>,
    predictor: Arc<Mutex<Predictor<f64>>>,
    precision_adjust: Arc<Mutex<PrecisionAdjust2>>,
    config: AutoAdjustLimits,
    traget_frequency: f64,
    freqmeter_config: Arc<Mutex<AdjustConfig>>,
) -> anyhow::Result<()> {
    const PRECISION_ADJ_ZAPAS: u32 = 3;

    // Стадия 1: поиск края
    *state.lock().await = State::DetctingEdge;
    display_progress(&status_report_q, "Поиск края".to_owned()).await?;

    let (initial_freq, edge_freq, last_freq_boxplot) = match find_edge(
        channel,
        update_interval_ms,
        &predictor,
        &precision_adjust,
        config.edge_detect_interval,
        status_report_q.clone(),
        (traget_frequency - config.min_freq_offset as f64).into(),
        traget_frequency.into(),
    )
    .await
    {
        Ok((edge_pos, start_freq, box_plot)) => {
            let end_freq = box_plot.median();
            tracing::warn!("Edge found at step {}", edge_pos);
            display_progress(
                &status_report_q,
                format!("Поиск края: {:.2} -> {:.2} Гц", start_freq, end_freq),
            )
            .await?;
            *state.lock().await = State::DihotomyStepping;
            (start_freq, end_freq, box_plot)
        }
        Err(e) => {
            tracing::error!("Edge not found: {}", e);
            status_report_q
                .send(AutoAdjustSingleStateReport::Error(e.to_string()))
                .await?;
            *state.lock().await = State::Idle;
            return Ok(());
        }
    };

    // Стадия 2: "Грубая" настройка
    let (new_state, fast_forward_end_freq, fast_forward_steps_used) = match do_fast_forward_adjust(
        traget_frequency,
        freqmeter_config.lock().await.working_offset_ppm as f64,
        last_freq_boxplot,
        &status_report_q,
        &precision_adjust,
        update_interval_ms,
        &predictor,
        channel,
        config.max_forward_steps - PRECISION_ADJ_ZAPAS,
        config.fast_forward_step_limit,
    )
    .await
    {
        Ok((state, freq, steps_used)) => {
            display_progress(
                &status_report_q,
                format!("Грубая настройка: -> {:.2} Гц ({} шага)", freq, steps_used),
            )
            .await?;
            (state, freq, steps_used)
        }
        Err(e) => {
            tracing::error!("Fast-forward failed: {}", e);
            status_report_q
                .send(AutoAdjustSingleStateReport::Error(e.to_string()))
                .await?;
            *state.lock().await = State::Idle;
            return Ok(());
        }
    };

    *state.lock().await = new_state;

    let (_new_state, precision_end_freq, precision_steps_used) =
        if new_state == State::PrecisionStepping {
            match do_precision_adjust(
                traget_frequency,
                freqmeter_config.lock().await.working_offset_ppm as f64,
                fast_forward_end_freq,
                config.max_forward_steps - fast_forward_steps_used,
                update_interval_ms,
                &status_report_q,
                &precision_adjust,
                &predictor,
                channel,
            )
            .await
            {
                Ok((state, freq, steps_used)) => {
                    display_progress(
                        &status_report_q,
                        format!("Точная настройка: -> {:.2} Гц ({} шага)", freq, steps_used),
                    )
                    .await?;
                    (state, Some(freq), steps_used)
                }
                Err(e) => {
                    tracing::error!("Precision adjust failed: {}", e);
                    status_report_q
                        .send(AutoAdjustSingleStateReport::Error(e.to_string()))
                        .await?;
                    *state.lock().await = State::Idle;
                    return Ok(());
                }
            }
        } else {
            display_progress(&status_report_q, "Точная настройка пропущена".to_owned()).await?;
            (new_state, None, 0)
        };

    // report
    {
        let precision_end_freq = precision_end_freq.unwrap_or(fast_forward_end_freq);

        let result = precision_end_freq;
        let offset_ppm = (result - traget_frequency) / traget_frequency * 1_000_000.0;
        let total_steps_used = fast_forward_steps_used + precision_steps_used;

        status_report_q
            .send(AutoAdjustSingleStateReport::Finished(format!(
                "Настройка завершена: {initial_freq:.2} -> {edge_freq:.2} -> {fast_forward_end_freq:.2} -> {precision_end_freq:.2} Гц ({offset_ppm:+.1} ppm) за {total_steps_used} шагов",
            )))
            .await?;
    }

    // success
    *state.lock().await = State::Idle;
    Ok(())
}

async fn display_progress(
    status_report_q: &mpsc::Sender<AutoAdjustSingleStateReport>,
    msg: String,
) -> Result<(), mpsc::error::SendError<AutoAdjustSingleStateReport>> {
    status_report_q
        .send(AutoAdjustSingleStateReport::Progress(msg))
        .await
}

async fn burn(
    precision_adjust: &Mutex<PrecisionAdjust2>,
    soft_mode: bool,
) -> Result<(), HardwareLogickError> {
    precision_adjust
        .lock()
        .await
        .burn(soft_mode)
        .await
        .map_err(|e| HardwareLogickError(format!("Не удалось включить лазер ({e:?})")))
}

async fn step(
    precision_adjust: &Mutex<PrecisionAdjust2>,
    count: i32,
) -> Result<(), laser_precision_adjust::Error> {
    precision_adjust.lock().await.step(count).await
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

async fn capture(predictor: &Mutex<Predictor<f64>>) -> (Vec<f64>, Option<u128>) {
    let capture = predictor.lock().await.capture().await;

    let data = capture.iter().map(|d| d.1 as f64).collect();
    let start_itmestamp = capture.first().map(|d| d.0);

    (data, start_itmestamp)
}

async fn find_edge(
    channel: u32,
    update_interval_ms: u32,
    predictor: &Mutex<Predictor<f64>>,
    precision_adjust: &Mutex<PrecisionAdjust2>,
    edge_detect_interval: u32,
    status_report_q: mpsc::Sender<AutoAdjustSingleStateReport>,
    min_frequency: f64,
    max_frequency: f64,
) -> anyhow::Result<(u32, f64, BoxPlot<f64>)> {
    use std::cmp::min;

    let mut start_freq = None;

    // switch channel
    precision_adjust
        .lock()
        .await
        .select_channel(channel)
        .await
        .map_err(|e| HardwareLogickError(format!("Не удалось переключить канал ({e:?})")))?;

    display_progress(&status_report_q, format!("Канал {channel}")).await?;

    // switch delay
    sleep_ms(min((update_interval_ms * 5) as u64, 500)).await;

    let mut current_step = 0;

    loop {
        // Прожиг
        burn(precision_adjust, false).await?;
        display_progress(
            &status_report_q,
            format!("Ожидаине реакции на шаге {current_step}"),
        )
        .await?;
        sleep_ms((update_interval_ms * 10) as u64).await;

        // поиск во фрагменте повышения частоты не менее чем на 0.35 Гц
        const EDGE_DETECT_PEAK: f64 = 0.35;
        {
            let (last_fragment, _) = capture(predictor).await;
            let box_plot = BoxPlot::new(&last_fragment);

            if start_freq.is_none() {
                start_freq.replace(box_plot.median());
            }

            if box_plot.q1() < min_frequency && current_step == 0 {
                Err(HardwareLogickError(format!(
                    "Частота ниже минимально-допустимой ({} < {:.2})",
                    min_frequency,
                    box_plot.q1()
                )))?
            } else if box_plot.q3() > max_frequency {
                Err(HardwareLogickError(format!(
                    "Частота выше максимально-допустимой ({} > {:.2})",
                    max_frequency,
                    box_plot.q3()
                )))?
            } else if (box_plot.q3() - box_plot.q1() > EDGE_DETECT_PEAK)
                && ((last_fragment.first().map_or(box_plot.q1(), |v| *v)
                    - last_fragment.last().map_or(box_plot.q3(), |v| *v))
                .abs()
                    > EDGE_DETECT_PEAK)
            {
                // нашли
                display_progress(&status_report_q, format!("Реакция обнаружена!")).await?;
                return Ok((current_step, start_freq.unwrap(), box_plot));
            }
        }

        // не найдено, шагаем на edge_detect_interval
        match step(precision_adjust, edge_detect_interval as i32).await {
            Ok(_) => {
                current_step += edge_detect_interval;
            } // ok
            Err(laser_precision_adjust::Error::Logick(_)) => break, // конец хода
            Err(e) => Err(HardwareLogickError(format!(
                "Не удалось сделать шаг ({e:?})"
            )))?,
        }
    }

    Err(HardwareLogickError(format!(
        "Край не найден, достигнут лимит перемещения ({current_step})"
    )))?
}

/// Итеративно делаем N шагов вперед, где N = (traget_frequency - forecast) / max_prediction
/// Если N < 1, то переходим к точной настройке
async fn do_fast_forward_adjust(
    traget_frequency: f64,
    precision_ppm: f64,
    last_freq_boxplot: BoxPlot<f64>,
    status_report_q: &mpsc::Sender<AutoAdjustSingleStateReport>,
    precision_adjust: &Mutex<PrecisionAdjust2>,
    update_interval_ms: u32,
    predictor: &Mutex<Predictor<f64>>,
    channel: u32,
    max_forward_steps: u32,
    step_limit: u32,
) -> Result<(State, f64, u32), anyhow::Error> {
    let f_lower_baund = traget_frequency * (1.0 - precision_ppm / 1_000_000.0);

    let mut total_step_counter: u32 = 0;
    let mut step_limit_over = false;

    let mut forecast = last_freq_boxplot.upper_bound();

    Ok(loop {
        // определяем сколько шагов нужно сделать, но не менее 1
        let mut steps_forecast = ((traget_frequency as f64 - forecast)
            / predictor
                .lock()
                .await
                .get_prediction(channel, 0.0)
                .await
                .unwrap()
                .maximal)
            .floor() as i32;
        tracing::trace!("New fast-forward forecast: {} steps", steps_forecast);
        if steps_forecast < 1 {
            // ожидание + переизмеренть частоту
            sleep_ms((update_interval_ms * 5) as u64).await;
            let current_freq = reread_freq(predictor).await;

            // ложное срабатывание?
            let fr = predictor
                .lock()
                .await
                .get_prediction(channel, current_freq)
                .await
                .unwrap()
                .median;
            if fr < f_lower_baund {
                forecast = current_freq;
                tracing::warn!(
                    "Fast-forward: detected false-stop: predict-median={:.2}, lb={:.2}",
                    fr,
                    f_lower_baund
                );
                continue;
            }

            // переход к точной настройке
            return Ok((State::PrecisionStepping, current_freq, total_step_counter));
        } else if steps_forecast > step_limit as i32 {
            tracing::warn!("Fast-forawrd: trimmed steps to {}", step_limit);
            steps_forecast = step_limit as i32;
        }

        // прожиг steps_forecast шагов
        status_report_q
            .send(AutoAdjustSingleStateReport::Progress(format!(
                "Прожиг {} шагов",
                steps_forecast
            )))
            .await?;
        let mut last_timestamp: Option<u128> = None;

        total_step_counter += steps_forecast as u32;
        if total_step_counter > max_forward_steps {
            steps_forecast -= (total_step_counter - max_forward_steps) as i32;
            total_step_counter = max_forward_steps;
            step_limit_over = true;
        }

        tracing::trace!("Burn {} steps...", steps_forecast);

        for _ in 0..steps_forecast {
            burn(&precision_adjust, false).await?;
            sleep_ms((update_interval_ms * 4) as u64).await;
            match step(&precision_adjust, 1).await {
                Ok(_) => {
                    let (_, ts) = capture(&predictor).await;
                    if let Some(ts) = ts {
                        last_timestamp.replace(ts);
                    }
                }
                Err(laser_precision_adjust::Error::Logick(_)) => {
                    Err(HardwareLogickError(
                        "Достигнут лимит перемещения, невозможно продолжить".to_owned(),
                    ))?;
                }
                Err(e) => Err(HardwareLogickError(format!(
                    "Не удалось сделать шаг ({e:?})"
                )))?,
            }
        }

        tracing::trace!("...Success");

        // защита от "залипания"
        if last_timestamp.is_none() {
            Err(HardwareLogickError(
                "Не удалось получить данные с частотмера, аварийный останов".to_owned(),
            ))?;
        }

        // ожидаем полного охлаждения
        display_progress(&status_report_q, "Ожидание охлаждения".to_owned()).await?;
        tracing::trace!("Cooling...");
        let last_fragment = loop {
            sleep_ms(update_interval_ms as u64).await;
            let last_fragment = get_last_fragment(&predictor, channel).await;
            if let Some(fragment) = &last_fragment {
                if fragment.start_timestamp() >= last_timestamp.unwrap() as f64 {
                    break last_fragment.unwrap();
                }
            }
        };

        // обновляем прогноз
        forecast = last_fragment.target();
        if step_limit_over {
            // Достигнут лимит шагов, принудительно выходим
            tracing::info!("Fast-forward: Sep limit");
            break (State::PrecisionStepping, forecast, total_step_counter);
        } else if forecast > traget_frequency {
            tracing::info!("Fast-forward: forecast ok");
            // прогноз показывает, что частота уже выше целевой, переходим к PrecisionStepping
            break (
                State::PrecisionStepping,
                reread_freq(predictor).await,
                total_step_counter,
            );
        } else if forecast > f_lower_baund {
            tracing::info!("Fast-forward: forecast upper min");
            // достигнута нижняя граница, останов грубой настройки, переходим к PrecisionStepping
            break (
                State::PrecisionStepping,
                reread_freq(predictor).await,
                total_step_counter,
            );
        } else {
            // продолжаем грубую настройку
            tracing::trace!("Need more fast-forward steps");
        }
    })
}

/// Итеративно делаем шаги вперед, пока прогноз не станет выше целевой частоты
async fn do_precision_adjust(
    traget_frequency: f64,
    precision_ppm: f64,
    mut current_freq: f64,
    max_steps: u32,
    update_interval_ms: u32,
    status_report_q: &mpsc::Sender<AutoAdjustSingleStateReport>,
    precision_adjust: &Mutex<PrecisionAdjust2>,
    predictor: &Mutex<Predictor<f64>>,
    channel: u32,
) -> Result<(State, f64, u32), anyhow::Error> {
    let f_lower_baund = traget_frequency * (1.0 - precision_ppm / 1_000_000.0);
    let f_lower_stop_baund = traget_frequency * (1.0 - (precision_ppm * 2.0 / 3.0) / 1_000_000.0);
    let f_upper_baund = traget_frequency * (1.0 + precision_ppm / 1_000_000.0);
    let mut total_step_counter: u32 = 0;

    let target_state = loop {
        if current_freq > f_lower_stop_baund {
            break State::End;
        }

        let forecast = predictor
            .lock()
            .await
            .get_prediction(channel, current_freq)
            .await
            .ok_or(HardwareLogickError("Отсутвует прогноз!".to_owned()))?;

        if forecast.maximal >= f_upper_baund {
            // Иногда прогноз дает очень большое преввышение, проверим на всякий случай
            current_freq = reread_freq(predictor).await;
            if current_freq > f_lower_baund {
                break State::End;
            }
        }

        if total_step_counter >= max_steps {
            // достигнут лимит шагов, принудительно выходим
            display_progress(&status_report_q, "Достигнут лимит шагов".to_owned()).await?;
            break State::End;
        }

        // прожиг 1 шага
        burn(&precision_adjust, true).await?;
        total_step_counter += 1;
        sleep_ms((update_interval_ms * 4) as u64).await;
        match step(&precision_adjust, 1).await {
            Ok(_) => {
                if let (_, Some(ts)) = capture(&predictor).await {
                    // ожидаем полного охлаждения
                    display_progress(&status_report_q, "Ожидание охлаждения".to_owned()).await?;
                    let last_fragment = loop {
                        sleep_ms(update_interval_ms as u64).await;
                        let last_fragment = get_last_fragment(&predictor, channel).await;
                        if let Some(fragment) = &last_fragment {
                            if fragment.start_timestamp() >= ts as f64 {
                                break last_fragment.unwrap();
                            }
                        }
                    };

                    // обновляем текущую частоту
                    current_freq = last_fragment.target();
                    display_progress(
                        &status_report_q,
                        format!("Текущая частота: ~{:.2} Гц", current_freq),
                    )
                    .await?;
                    if current_freq > f_lower_stop_baund {
                        // на всякий случай
                        sleep_ms((update_interval_ms * 5) as u64).await;
                        current_freq = reread_freq(predictor).await;
                    }
                } else {
                    Err(HardwareLogickError(
                        "Не удалось получить данные с частотмера, аварийный останов".to_owned(),
                    ))?;
                }
            }
            Err(laser_precision_adjust::Error::Logick(_)) => {
                Err(HardwareLogickError(
                    "Достигнут лимит перемещения, невозможно продолжить".to_owned(),
                ))?;
            }
            Err(e) => Err(HardwareLogickError(format!(
                "Не удалось сделать шаг ({e:?})"
            )))?,
        }
    };

    // ожидание + переизмеренть частоту
    sleep_ms((update_interval_ms * 5) as u64).await;
    current_freq = reread_freq(predictor).await;

    Ok((target_state, current_freq, total_step_counter))
}

async fn reread_freq(predictor: &Mutex<Predictor<f64>>) -> f64 {
    predictor.lock().await.capture_next_point().await as f64
}
