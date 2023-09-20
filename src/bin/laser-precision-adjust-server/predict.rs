use std::{marker::PhantomData, sync::Arc};

use laser_precision_adjust::{ForecastConfig, Status};
use num_traits::Float;
use serde::{Serialize, Deserialize};
use tokio::sync::{watch::Receiver, Mutex};

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

impl<T: Float + num_traits::FromPrimitive + 'static> Predictor<T> {
    pub fn new(rx: Receiver<Status>, forecast_config: ForecastConfig) -> Self {
        let fragments = Arc::new(Mutex::new(vec![]));

        {
            let fragments = fragments.clone();
            tokio::spawn(Self::task(rx, fragments));
        }
        Self {
            fragments,
            forecast_config,
            _t: PhantomData::<T>,
        }
    }

    async fn task(mut status_rx: Receiver<Status>, fragments: Arc<Mutex<Vec<Vec<Fragment<T>>>>>) {
        loop {
            status_rx.changed().await.ok();
        }
    }

    /// получить все удачно-аппроксикированные фрагменты
    /// не старше указанного времени (или вообще все, если время не указано)
    async fn get_fragments(&self, channel: u32, t_min: Option<u128>) -> Vec<Fragment<T>> {
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
    async fn get_prediction(&self, _channel: u32, f_start: T) -> Prediction<T> {
        // Static prediction
        unsafe {
            Prediction {
                minimal: T::from_f32(self.forecast_config.min_freq_grow).unwrap_unchecked()
                    + f_start,
                maximal: T::from_f32(self.forecast_config.max_freq_grow).unwrap_unchecked()
                    + f_start,
                median: T::from_f32(self.forecast_config.median_freq_grow).unwrap_unchecked()
                    + f_start,
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Fragment<T> {
    start_timestamp: u128,
    raw_points: Vec<DataPoint<T>>,
}

impl<T: Float> Fragment<T> {
    // Создать фрагмент из набора точек
    // start_timestamp - время начала фрагмента
    // raw_points - набор точек
    // min_grow_speed - минимальная скорость роста
    // max_grow_speed - максимальная скорость роста
    pub fn new(
        start_timestamp: u128,
        raw_points: &[DataPoint<T>],
        min_grow_speed: T,
        max_grow_speed: T,
    ) -> Self {
        Self {
            start_timestamp,
            raw_points: raw_points.to_vec(),
        }
    }

    pub fn points(&self) -> &[DataPoint<T>] {
        &self.raw_points
    }

    // Найти индекс точки, в которой достигается минимум raw_points
    pub fn minimum_index(&self) -> usize {
        todo!()
    }

    // Коэффициенты аппроксимации функцией y = A * (1 - exp(-x * B))
    // Принимается, что исходная кривя смещена таким образом, что минимум
    // находится в точке (0, 0)
    // Если аппроксимация не удалась, возвращается None
    pub fn aprox_coeffs(&self) -> Option<(T, T)> {
        todo!()
    }

    // Рассчитать аппроксимированную кривую начинаи подвинуть
    // её в точку (x_start, y_offset)
    // Если аппроксимация не удалась, возвращается None
    pub fn evaluate(&self, x_start: T, y_offset: T) -> Option<Vec<(T, T)>> {
        todo!()
    }
}

unsafe impl<T: Float> Send for Fragment<T> {}


#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
pub struct FullDataPoint<T: Float> {
    pub x: T,
    pub y: T,
    //pub dy: T,
    //pub d2y: T,
}

fn filter_points<T, U>(points: &[U]) -> Vec<Option<FullDataPoint<T>>>
where
    T: num_traits::Float + num_traits::FromPrimitive + csaps::Real,
    U: IDataPoint<T> + Clone,
{
    use laser_precision_adjust::box_plot::BoxPlot;

    let pg = unsafe { T::from_f64(0.85).unwrap_unchecked() };

    let x = points.iter().map(|p| p.x()).collect::<Vec<_>>();
    let mut y = points.iter().map(|p| p.y()).collect::<Vec<_>>();
    let mut prev_y = None;

    // первичная фильтрация, должна отрезать точки ~0
    let raw_box_plot = BoxPlot::new(&y);
    y.iter_mut().for_each(|y| {
        if (*y > raw_box_plot.lower_bound()) && (*y < raw_box_plot.upper_bound()) {
            // ok
            prev_y = Some(*y);
        } else {
            if let Some(py) = prev_y {
                *y = py;
            }
        }
    });

    // Апроксимация сплайном
    if let Ok(spline) = csaps::CubicSmoothingSpline::new(&x, &y)
        .with_smooth(pg)
        .make()
    {
        // вычисление сплайна в точках
        if let Ok(spline_y_vals) = spline.evaluate(&x) {
            // разница истинного значения и прогноза
            let diffs = y
                .iter()
                .zip(spline_y_vals.iter())
                .map(|(y, ys)| *y - *ys)
                .collect::<Vec<_>>();

            let box_plot = BoxPlot::new(&diffs);

            // вторичный фильтр, удаляет случайные иголки вверх
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

            // повторная апроксимация сплайном
            if let Ok(spline) = csaps::CubicSmoothingSpline::new(&x, &y)
                .with_smooth(pg)
                .make()
            {
                if let Ok(y) = spline.evaluate(&x) {
                    let start = x.len().checked_sub(30).unwrap_or_default();
                    return x
                        .iter()
                        .enumerate()
                        .take(x.len() - 1)
                        .zip(y.into_iter())
                        .map(move |((i, x), y)| {
                            if i < start {
                                None
                            } else {
                                Some(FullDataPoint { x: *x, y: *y })
                            }
                        })
                        .collect();
                }
            }
        }
    }

    vec![]
}
