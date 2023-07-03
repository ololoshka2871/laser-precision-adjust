
pub enum GCodeCtrl {
    Reset,
    Setup,
    G0{ x: f32, y: f32 },
    M3{ s: f32 },
    M5,
    G1{ x: f32, y: f32, f: f32 },
}