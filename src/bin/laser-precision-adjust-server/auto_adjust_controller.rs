#[derive(PartialEq)]
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

}

impl AutoAdjestController {
    pub fn try_start(&mut self) -> Result<tokio::sync::mpsc::Receiver<u32>, &'static str>  {
        Err("Not implemented")
    }

    pub fn cancel(&mut self) -> Result<(), &'static str> {
        Err("Not implemented")
    }

    pub fn current_state(&self) -> State {
        State::Idle
    }
}