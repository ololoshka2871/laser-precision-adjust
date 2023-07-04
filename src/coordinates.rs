#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Left,
    Right,
}

pub trait CoordiantesCalc {
    fn to_abs(&self, step: u32, current_side: Side, total_steps: usize) -> (f32, f32);
}

impl CoordiantesCalc for crate::config::ResonatroPlacement {
    fn to_abs(&self, step: u32, current_side: Side, total_steps: usize) -> (f32, f32) {
        (
            if current_side == Side::Left { self.x - self.w / 2.0 } else { self.x + self.w / 2.0 },
            self.y + step as f32 * (self.h / total_steps as f32),
        )
    }
}
