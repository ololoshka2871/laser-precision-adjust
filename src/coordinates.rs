use crate::config::AxisConfig;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn mirrored(self) -> Self {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

pub trait CoordiantesCalc {
    fn to_abs(
        &self,
        axis_config: &AxisConfig,
        step: u32,
        current_side: Side,
        total_steps: u32,
    ) -> (f32, f32);
}

impl CoordiantesCalc for crate::config::ResonatroPlacement {
    fn to_abs(
        &self,
        axis_config: &AxisConfig,
        step: u32,
        side: Side,
        total_steps: u32,
    ) -> (f32, f32) {
        let (x, y) = (
            if (side == Side::Left) ^ axis_config.reverse_x {
                self.x - self.w / 2.0
            } else {
                self.x + self.w / 2.0
            },
            if axis_config.reverse_y {
                self.y - step as f32 * (self.h / total_steps as f32)
            } else {
                self.y + step as f32 * (self.h / total_steps as f32)
            },
        );

        if axis_config.swap_xy {
            (y, x)
        } else {
            (x, y)
        }
    }
}
