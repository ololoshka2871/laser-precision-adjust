use std::{sync::Arc, time::Duration};

use tokio::{sync::Mutex, time};

use laser_precision_adjust::AutoAdjustLimits;

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

pub struct AutoAdjestController {
    config: AutoAdjustLimits,
    state: Arc<Mutex<State>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl AutoAdjestController {
    pub fn new(config: AutoAdjustLimits) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(State::Idle)),
            task: None,
        }
    }

    pub async fn try_start(
        &mut self,
        channel: u32,
    ) -> Result<tokio::sync::mpsc::Receiver<u32>, &'static str> {
        if *self.state.lock().await == State::Idle {
            let (tx, rx) = tokio::sync::mpsc::channel(1);

            tracing::warn!("Start auto-adjustion channel {}", channel);
            self.task
                .replace(tokio::spawn(Self::adjust_task(tx, self.state.clone())));

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

    async fn adjust_task(
        status_report_q: tokio::sync::mpsc::Sender<u32>,
        state: Arc<Mutex<State>>,
    ) {
        *state.lock().await = State::DetctingEdge;
        for i in 0..10 {
            match status_report_q.send(i).await {
                Ok(_) => time::sleep(Duration::from_secs(1)).await,
                Err(e) => {
                    tracing::error!("adjust_task error: {}", e);
                    break;
                }
            }
        }
        *state.lock().await = State::Idle;
    }
}
