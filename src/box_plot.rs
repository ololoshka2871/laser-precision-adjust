use num_traits::{Float, FromPrimitive, NumOps};

pub struct BoxPlot<T> {
    median: T,
    q1: T,
    q3: T,
    iqr: T,
    lower_bound: T,
    upper_bound: T,
}

// Функция вычисления медианы вектора и квартилей 25% и 75%
fn median_q1q3<T>(series: &[T]) -> (T, T, T)
where
    T: Float + Copy,
{
    let mut sorted_series = series
        .into_iter()
        .filter(|v| !v.is_nan())
        .copied()
        .collect::<Vec<_>>();
    if sorted_series.is_empty() {
        (T::nan(), T::nan(), T::nan())
    } else {
        sorted_series.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let len = sorted_series.len();
        let q1 = sorted_series[len / 4];
        let q3 = sorted_series[len * 3 / 4];
        let median = sorted_series[len / 2];
        (median, q1, q3)
    }
}

#[allow(unused)]
impl<T> BoxPlot<T>
where
    T: Float + NumOps + FromPrimitive + Copy,
{
    pub fn new(series: &[T]) -> Self {
        let poltora = unsafe { T::from_f64(2.0).unwrap_unchecked() };

        let (median, q1, q3) = median_q1q3(series);
        let iqr = q3 - q1;
        let lower_bound = q1 - poltora * iqr;
        let upper_bound = q3 + poltora * iqr;
        Self {
            median,
            q1,
            q3,
            iqr,
            lower_bound,
            upper_bound,
        }
    }

    pub fn median(&self) -> T {
        self.median
    }

    pub fn q1(&self) -> T {
        self.q1
    }

    pub fn q3(&self) -> T {
        self.q3
    }

    pub fn iqr(&self) -> T {
        self.iqr
    }

    pub fn lower_bound(&self) -> T {
        self.lower_bound
    }

    pub fn upper_bound(&self) -> T {
        self.upper_bound
    }

    pub fn bound(&self, m: T) -> T {
        (if m < T::zero() { self.q1 } else { self.q3 }) + m * self.iqr
    }
}
