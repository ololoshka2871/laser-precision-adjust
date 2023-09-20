use std::{marker::PhantomData, sync::Arc};

use laser_precision_adjust::{box_plot::BoxPlot, ForecastConfig, Status};
use num_traits::Float;
use serde::{Deserialize, Serialize};
use tokio::sync::{watch::Receiver, Mutex};
use tracing_subscriber::registry::Data;

use crate::{DataPoint, IDataPoint};

#[derive(Clone, Copy, Debug)]
pub struct Prediction<T: Float> {
    pub minimal: T,
    pub maximal: T,
    pub median: T,
}

pub struct Predictor<T> {
    fragments: Arc<Mutex<Vec<Vec<Fragment<T>>>>>,
    forecast_config: ForecastConfig,
    _t: PhantomData<T>,
}

impl<T: Float + num_traits::FromPrimitive + csaps::Real + 'static> Predictor<T> {
    pub fn new(
        rx: Receiver<Status>,
        forecast_config: ForecastConfig,
        channels_count: usize,
        fragment_len: usize,
    ) -> Self {
        let fragments = Arc::new(Mutex::new(vec![vec![]; channels_count]));

        {
            let fragments = fragments.clone();
            tokio::spawn(Self::task(rx, fragments, fragment_len));
        }
        Self {
            fragments,
            forecast_config,
            _t: PhantomData::<T>,
        }
    }

    async fn task(
        mut status_rx: Receiver<Status>,
        fragments: Arc<Mutex<Vec<Vec<Fragment<T>>>>>,
        fragment_len: usize,
    ) {
        let mut current_chanel = None;
        let mut serie_data = vec![];
        loop {
            status_rx.changed().await.ok();

            let new_status = status_rx.borrow().clone();

            if new_status.shot_mark {
                // выстрел - фиксируем канал
                current_chanel.replace(new_status.current_channel);

                // drop data
                serie_data.clear();

                serie_data.push((
                    new_status.since_start.as_millis(),
                    new_status.current_frequency,
                ))
            } else if current_chanel != Some(new_status.current_channel) {
                // произошла смена канала, выстрелов не было, так что снимаем определенность канала.
                current_chanel = None;
                // drop data
                serie_data.clear();
            } else {
                // продолжаем обработку текущего окна
                if current_chanel.is_some() && serie_data.len() < fragment_len {
                    serie_data.push((
                        new_status.since_start.as_millis(),
                        new_status.current_frequency,
                    ))
                } else {
                    let cc = current_chanel.unwrap() as usize;
                    // сброс
                    current_chanel = None;

                    // Сбор закончен
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
                        if let Some((f_min_index, min_f)) = find_min(&f) {
                            let fz = filtred_f
                                .iter()
                                .skip(f_min_index - 1)
                                .map(move |f| *f - min_f)
                                .collect::<Vec<_>>();

                            // Апроксимация экспонентой
                            if let Ok(coeffs) = aproximate_exp(&t[f_min_index..], &fz) {
                                let mut guard = fragments.lock().await;
                                let serie =
                                    guard.get_mut(cc).unwrap();
                                let data = t
                                    .iter()
                                    .zip(f)
                                    .map(|(t, f)| DataPoint::new(*t, f))
                                    .collect::<Vec<_>>();
                                serie.push(Fragment::new(
                                    serie_data[0].0,
                                    &data,
                                    coeffs,
                                    f_min_index,
                                ))
                            } else {
                                tracing::error!("Failed to aproximate last fragment");
                            }
                        }
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
}

#[derive(Clone)]
pub struct Fragment<T> {
    start_timestamp: f64,
    raw_points: Vec<DataPoint<T>>,
    coeffs: (T, T),
    min_index: usize,
}

impl<T: Float> Fragment<T> {
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
        self.raw_points.clone()
    }
}

unsafe impl<T: Float> Send for Fragment<T> {}

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

fn find_min<T: Float>(data: &[T]) -> Option<(usize, T)> {
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

fn aproximate_exp<T: Float>(x: &[T], y: &[T]) -> Result<(T, T), ()> {
    Ok((T::zero(), T::zero()))
}
