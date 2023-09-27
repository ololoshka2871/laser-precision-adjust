use std::{fmt::Debug, marker::PhantomData, sync::Arc};

use nalgebra::{DVector, Scalar};
use num_traits::Float;
use serde::Serialize;
use tokio::sync::{watch::Receiver, Mutex};

use crate::{box_plot::BoxPlot, DataPoint, ForecastConfig, IDataPoint, Status};

#[derive(Clone, Copy, Debug)]
pub struct Prediction<T: Float> {
    pub minimal: T,
    pub maximal: T,
    pub median: T,
}

pub struct Predictor<T: Serialize> {
    fragments: Arc<Mutex<Vec<Vec<Fragment<T>>>>>,
    forecast_config: ForecastConfig,
    serie_data: Arc<Mutex<Vec<(u128, f32)>>>,
    _t: PhantomData<T>,
}

pub const NORMAL_T: f64 = 1000.0;

impl<T> Predictor<T>
where
    T: Float + num_traits::FromPrimitive + csaps::Real + nalgebra::RealField + Serialize + 'static,
{
    pub fn new(
        rx: Receiver<Status>,
        forecast_config: ForecastConfig,
        channels_count: usize,
        fragment_len: usize,
    ) -> Self {
        let fragments = Arc::new(Mutex::new(vec![vec![]; channels_count]));
        let serie_data = Arc::new(Mutex::new(vec![]));

        {
            let fragments = fragments.clone();
            let serie_data = serie_data.clone();
            tokio::spawn(Self::task(rx, fragments, fragment_len, serie_data));
        }
        Self {
            fragments,
            forecast_config,
            serie_data,
            _t: PhantomData::<T>,
        }
    }

    pub async fn save(&self, path: Option<std::path::PathBuf>) -> std::io::Result<()> {
        if let Some(data_log_file_name) = path {
            let file = std::fs::File::create({
                let now = chrono::offset::Local::now();
                let path_main = now.format(data_log_file_name.to_str().unwrap()).to_string();
                format!("{path_main}-fragments.json")
            })?;

            let fragments = self.fragments.lock().await;
            serde_json::to_writer_pretty::<_, Vec<Vec<Fragment<T>>>>(file, fragments.as_ref())?;
        }
        Ok(())
    }

    pub async fn reset(&mut self) {
        self.fragments
            .lock()
            .await
            .iter_mut()
            .for_each(|ch| ch.clear());
    }

    async fn task(
        mut status_rx: Receiver<Status>,
        fragments: Arc<Mutex<Vec<Vec<Fragment<T>>>>>,
        fragment_len: usize,
        serie_data: Arc<Mutex<Vec<(u128, f32)>>>,
    ) {
        let mut current_chanel = None;
        loop {
            status_rx.changed().await.ok();

            let new_status = status_rx.borrow().clone();

            if new_status.shot_mark {
                // выстрел - фиксируем канал
                current_chanel.replace(new_status.current_channel);

                // Сбор прерван
                {
                    let mut guard = serie_data.lock().await;
                    if guard.len() > 3 {
                        if let Err(_) = try_consume_fragment(
                            &guard,
                            new_status.current_channel as usize,
                            &fragments,
                        )
                        .await
                        {
                            tracing::error!("Invalid fragment");
                        }
                    }
                    // drop data
                    guard.clear();
                }
            } else if current_chanel != Some(new_status.current_channel) {
                // произошла смена канала, выстрелов не было, так что снимаем определенность канала.
                current_chanel = None;
                // drop data
                serie_data.lock().await.clear();
            } else {
                // продолжаем обработку текущего окна
                let mut guard = serie_data.lock().await;
                if current_chanel.is_some() && guard.len() < fragment_len {
                    guard.push((
                        new_status.since_start.as_millis(),
                        new_status.current_frequency,
                    ))
                } else {
                    let cc = current_chanel.unwrap() as usize;

                    // сброс
                    current_chanel = None;

                    // Сбор закончен
                    if let Err(_) = try_consume_fragment(&guard, cc, &fragments).await {
                        tracing::error!("Failed to aproximate last fragment");
                    }
                }
            }
        }
    }

    /// получить все удачно-аппроксикированные фрагменты
    /// не старше указанного времени (или вообще все, если время не указано)
    pub async fn get_fragments(&self, channel: u32, t_min: Option<f64>) -> Vec<Fragment<T>> {
        let guard = self.fragments.lock().await;
        if let Some(channel_data) = guard.get(channel as usize) {
            if let Some(t_min) = t_min {
                let mut res = channel_data
                    .iter()
                    .rev()
                    .take_while(|f| f.start_timestamp >= t_min)
                    .cloned()
                    .collect::<Vec<_>>();
                res.reverse();
                res
            } else {
                channel_data.clone()
            }
        } else {
            vec![]
        }
    }

    /// Получить последний фрагмент канала, если есть
    pub async fn get_last_fragment(&self, channel: u32) -> Option<Fragment<T>> {
        let guard = self.fragments.lock().await;
        if let Some(channel_data) = guard.get(channel as usize) {
            channel_data.last().cloned()
        } else {
            None
        }
    }

    /// Получить прогноз для изменения частоты для текущего канала если произвести
    /// выстрел сейчас
    pub async fn get_prediction(&self, _channel: u32, f_start: T) -> Option<Prediction<T>> {
        // Static prediction
        unsafe {
            Some(Prediction {
                minimal: T::from_f32(self.forecast_config.min_freq_grow).unwrap_unchecked()
                    + f_start,
                maximal: T::from_f32(self.forecast_config.max_freq_grow).unwrap_unchecked()
                    + f_start,
                median: T::from_f32(self.forecast_config.median_freq_grow).unwrap_unchecked()
                    + f_start,
            })
        }
    }

    pub async fn capture(&self) -> Vec<(u128, f32)> {
        self.serie_data.lock().await.clone()
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Fragment<T: Serialize> {
    start_timestamp: f64,
    raw_points: Vec<DataPoint<T>>,
    coeffs: (T, T),
    min_index: usize,
}

impl<T: Serialize> Fragment<T>
where
    T: Float
        + num_traits::FromPrimitive
        + nalgebra::Scalar
        + std::ops::MulAssign
        + std::ops::AddAssign
        + std::ops::DivAssign,
{
    // Создать фрагмент из набора точек
    // start_timestamp - время начала фрагмента
    // raw_points - набор точек
    // coeffs - коэфициенты
    // min_index - индекс точки минимума
    pub fn new(
        start_timestamp: u128,
        raw_points: &[DataPoint<T>],
        coeffs: (T, T),
        min_index: usize,
    ) -> Self {
        Self {
            start_timestamp: start_timestamp as f64,
            raw_points: raw_points.to_vec(),
            coeffs,
            min_index,
        }
    }

    pub fn points(&self) -> &[DataPoint<T>] {
        &self.raw_points
    }

    // Найти индекс точки, в которой достигается минимум raw_points
    pub fn minimum_index(&self) -> usize {
        self.min_index
    }

    // Коэффициенты аппроксимации функцией y = A * (1 - exp(-x * B))
    // Принимается, что исходная кривя смещена таким образом, что минимум
    // находится в точке (0, 0)
    pub fn aprox_coeffs(&self) -> (T, T) {
        self.coeffs
    }

    // Рассчитать аппроксимированную кривую начинаи подвинуть
    // её в точку (x_start, y_offset)
    pub fn evaluate(&self) -> Vec<DataPoint<T>> {
        let normal_t = unsafe { T::from_f64(NORMAL_T).unwrap_unchecked() };

        self.raw_points
            .iter()
            .take(self.min_index)
            .cloned()
            .chain({
                let x_start = self.raw_points[self.min_index].x;
                let mut x = nalgebra::DVector::<T>::from_iterator(
                    self.raw_points.len() - self.min_index,
                    self.raw_points.iter().skip(self.min_index).map(|p| p.x),
                );
                x.add_scalar_mut(-x_start);
                x /= normal_t;
                let y = (limit_exp(&x, self.coeffs.1) * self.coeffs.0)
                    .add_scalar(self.raw_points[self.min_index].y);

                x.iter()
                    .zip(y.iter())
                    .map(|(x, y)| DataPoint::new(x_start + (*x) * normal_t, *y))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    // Таймштамп начала фрагмента
    pub fn start_timestamp(&self) -> f64 {
        self.start_timestamp
    }

    pub fn box_plot(&self) -> BoxPlot<T> {
        BoxPlot::new(&self.raw_points.iter().map(|p| p.y()).collect::<Vec<_>>())
    }
}

unsafe impl<T: Float + Serialize> Send for Fragment<T> {}

//-----------------------------------------------------------------------------

fn hard_filter<T: Float + num_traits::NumOps + num_traits::FromPrimitive + Copy>(data: &mut [T]) {
    let raw_box_plot = BoxPlot::new(data);
    let mut prev_y = None;

    data.iter_mut().for_each(|y| {
        if (*y > raw_box_plot.lower_bound()) && (*y < raw_box_plot.upper_bound()) {
            prev_y = Some(*y);
        } else if let Some(py) = prev_y {
            *y = py;
        }
    });
}

fn smooth_filter<'a, T>(x: &Vec<T>, y: &Vec<T>) -> csaps::Result<Vec<T>>
where
    T: Float + num_traits::NumOps + num_traits::FromPrimitive + csaps::Real + Copy,
{
    let pg = unsafe { T::from_f64(0.85).unwrap_unchecked() };
    let mut y = y.clone();

    // Апроксимация сплайном
    let spline = csaps::CubicSmoothingSpline::new(x, &y)
        .with_smooth(pg)
        .make()?;

    // вычисление сплайна в точках
    let spline_y_vals = spline.evaluate(&x)?;

    // разница истинного значения и прогноза
    let diffs = y
        .iter()
        .zip(spline_y_vals.iter())
        .map(|(y, ys)| *y - *ys)
        .collect::<Vec<_>>();

    let box_plot = BoxPlot::new(&diffs);

    // вторичный фильтр, удаляет случайные иголки вверх
    let mut prev_y = None;
    y.iter_mut().zip(diffs).for_each(move |(y, d)| {
        if (d > box_plot.lower_bound()) && (d < box_plot.upper_bound()) {
            // ok
            prev_y = Some(*y);
        } else {
            if let Some(py) = prev_y {
                *y = py;
            }
        }
    });

    Ok(y)
}

pub fn find_min<T: Float>(data: &[T]) -> Option<(usize, T)> {
    if let Some((mut min, mut minindex)) = data.first().map(|d0| (*d0, 0)) {
        data.iter().enumerate().for_each(|(i, v)| {
            if *v < min {
                min = *v;
                minindex = i;
            }
        });
        Some((minindex, min))
    } else {
        None
    }
}

pub fn aproximate_exp<T>(x: Vec<T>, y: &[T]) -> Result<(T, T), ()>
where
    T: Scalar + Float + nalgebra::ComplexField + nalgebra::RealField,
{
    use varpro::model::SeparableModel;
    use varpro::prelude::*;
    use varpro::solvers::levmar::{LevMarProblemBuilder, LevMarSolver};

    // на вход требуются матрицы из vec![] придется делать копии
    let x = nalgebra::DVector::<T>::from_vec(x);
    let y = nalgebra::DVector::<T>::from_vec(y.to_vec());

    let model = SeparableModelBuilder::<T>::new(&["b"]) // названия параметров модели
        .independent_variable(x) // переменная
        .function(&["b"], limit_exp) // функция, которй будем апроксимировать
        .partial_deriv("b", dlimit_exp_db) // частная производная по параметру b
        .initial_parameters(vec![unsafe { T::from_f64(1e-5).unwrap_unchecked() }]) // начальные значения параметров
        .build()
        .unwrap();
    // 2. Cast the fitting problem as a nonlinear least squares minimization problem
    let problem = LevMarProblemBuilder::<SeparableModel<T>>::new(model)
        .observations(y)
        .build()
        .unwrap();
    // 3. Solve the fitting problem
    let (solved_problem, report) = LevMarSolver::new().minimize(problem);
    if !report.termination.was_successful() {
        return Err(());
    } else {
        let b = solved_problem.params()[0];
        let a = solved_problem.linear_coefficients().unwrap()[0];
        Ok((a, b))
    }
}

//-----------------------------------------------------------------------------

async fn try_consume_fragment<T>(
    serie_data: &[(u128, f32)],
    channel: usize,
    fragments: &Mutex<Vec<Vec<Fragment<T>>>>,
) -> Result<(), ()>
where
    T: Float
        + num_traits::NumOps
        + num_traits::FromPrimitive
        + csaps::Real
        + nalgebra::RealField
        + Copy
        + Serialize,
{
    let normal_t = unsafe { T::from_f64(NORMAL_T).unwrap_unchecked() };
    let mut t = vec![];
    let mut f = vec![];
    serie_data.iter().for_each(|v| {
        t.push(unsafe { T::from_u128(v.0).unwrap_unchecked() });
        f.push(unsafe { T::from_f32(v.1).unwrap_unchecked() });
    });

    // Грубая фильтрация от всяких выбросов в 0
    hard_filter(&mut f);

    // сглаживающая фильтрация
    if let Ok(filtred_f) = smooth_filter(&t, &f) {
        if let Some((f_min_index, min_f)) = find_min(&filtred_f) {
            let fz = filtred_f
                .iter()
                .skip(f_min_index)
                .map(move |f| *f - min_f)
                .collect::<Vec<_>>();

            // Апроксимация экспонентой
            let t_zero = t[f_min_index];
            if let Ok(coeffs) = aproximate_exp(
                t[f_min_index..]
                    .iter()
                    .map(move |t| (*t - t_zero) / normal_t)
                    .collect::<Vec<_>>(),
                &fz,
            ) {
                const LIMIT_A: f64 = 5.0;

                if coeffs.0 > unsafe { T::from_f64(LIMIT_A).unwrap_unchecked() }
                    || coeffs.0 < T::zero()
                    || coeffs.1 < T::zero()
                {
                    Err(())?;
                } else {
                    tracing::trace!("Aprox fragment: a={}, b={}", coeffs.0, coeffs.1);
                }

                let mut guard = fragments.lock().await;
                let serie = guard.get_mut(channel).unwrap();
                let data = t
                    .iter()
                    .zip(f)
                    .map(|(t, f)| DataPoint::new(*t, f))
                    .collect::<Vec<_>>();
                serie.push(Fragment::new(serie_data[0].0, &data, coeffs, f_min_index));
                return Ok(());
            } else {
                tracing::warn!("Fragment approximation failed!");
            }
        }
    }

    Err(())
}

//-----------------------------------------------------------------------------

// Экспонента
pub fn limit_exp<T: Scalar + Float>(x: &DVector<T>, b: T) -> DVector<T> {
    x.map(|x| T::one() - (-x * b).exp())
}

// Производная d(limit_exp)/db
pub fn dlimit_exp_db<T: Scalar + Float>(dx: &DVector<T>, b: T) -> DVector<T> {
    dx.map(|x| x * (-x * b).exp())
}

//-----------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use varpro::prelude::*;
    use varpro::solvers::levmar::{LevMarProblemBuilder, LevMarSolver};

    use super::*;

    #[test]
    fn fit() {
        // оригинальная функция для апроксимации: a * (1 - exp(-x * b))
        // однако, библиотака работает с функциями вида A1 * f1(x, ...) + A2 * f2(x, ...) + ...
        // An - линейные коэфициенты, надо чтобы в fn их не было внутри, поэтому наша функция упрощаяется до 1 - exp(-x * b),
        // а линейный коэфициент a == solved_problem.linear_coefficients()[0]

        let orig_a = 1.0;
        let orig_b = 0.01;

        let x = nalgebra::DVector::<f64>::from_iterator(20, (0..200).step_by(10).map(|v| v as f64));
        let y_origin = limit_exp(&x, orig_b) * orig_a;
        let y_test = nalgebra::DVector::<f64>::from_iterator(
            y_origin.len(),
            y_origin
                .iter()
                .enumerate()
                .map(|(i, y)| y + -1e-3 * (i % 2) as f64),
        );

        // 1. Создание модели для апроксимации
        let model = SeparableModelBuilder::<f64>::new(&["b"]) // названия параметров модели
            .independent_variable(x) // переменная
            .function(&["b"], limit_exp) // функция, которй будем апроксимировать
            .partial_deriv("b", dlimit_exp_db) // частная производная по параметру b
            .initial_parameters(vec![1.0]) // начальные значения параметров
            .build()
            .unwrap();
        // 2. Cast the fitting problem as a nonlinear least squares minimization problem
        let problem = LevMarProblemBuilder::new(model)
            .observations(y_test)
            .build()
            .unwrap();
        // 3. Solve the fitting problem
        let (solved_problem, report) = LevMarSolver::new().minimize(problem);
        assert!(report.termination.was_successful());
        // 4. obtain the nonlinear parameters after fitting
        let alpha = solved_problem.params();
        print!("{alpha}");
        // 5. obtain the linear parameters
        let c = solved_problem.linear_coefficients().unwrap();
        print!("{c}")
    }

    #[test]
    fn fit_real_data() {
        let x =
            nalgebra::DVector::<f64>::from_vec(vec![0., 105.0, 211., 317., 423., 528., 633., 738.])
                / NORMAL_T;
        let y = nalgebra::DVector::<f64>::from_vec(vec![
            0.0,
            0.03125,
            0.162109375,
            0.26171875,
            0.29296875,
            0.359375,
            0.435546875,
            0.435546876,
        ]);

        let model = SeparableModelBuilder::<f64>::new(&["b"]) // названия параметров модели
            .independent_variable(x) // переменная
            .function(&["b"], limit_exp) // функция, которй будем апроксимировать
            .partial_deriv("b", dlimit_exp_db) // частная производная по параметру b
            .initial_parameters(vec![1.0]) // начальные значения параметров
            .build()
            .unwrap();
        let problem = LevMarProblemBuilder::new(model)
            .observations(y)
            .build()
            .unwrap();

        let (solved_problem, report) = LevMarSolver::new().minimize(problem);
        assert!(report.termination.was_successful());
        let b = solved_problem.params()[0];
        let a = solved_problem.linear_coefficients().unwrap()[0];
        println!("a = {a}, b = {b}")
    }
}
