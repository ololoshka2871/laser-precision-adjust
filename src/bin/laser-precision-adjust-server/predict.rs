use std::marker::PhantomData;


pub struct Predictor<T> {
    _t: PhantomData<T>
}

impl<T: num_traits::Float> Predictor<T> {
    pub fn new() -> Self {
        Self {
            _t: PhantomData::<T>,
        }
    }
}