use std::fmt::Display;

#[derive(Debug, Clone)]
pub enum GCodeCtrl {
    /// Reset to initial state
    Reset,

    /// Stop moving and turn off laser, set laser pump power to a
    Setup { a: f32 },

    /// Send raw GCode command
    Raw(String),

    /// Simple Move to x, y
    G0 { x: f32, y: f32 },

    /// Turn on laser with power s
    M3 { s: f32 },

    /// Turn off laser
    M5,

    /// Move to x, y with feedrate f
    G1 { x: f32, y: f32, f: f32 },
}

impl Display for GCodeCtrl {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GCodeCtrl::Reset => writeln!(fmt, "M5\nG90\nG0 X0Y0"),
            GCodeCtrl::Setup { a } => writeln!(fmt, "G90\nM5\nG1 A{}", a),
            GCodeCtrl::Raw(s) => writeln!(fmt, "{}", s),
            GCodeCtrl::G0 { x, y } => writeln!(fmt, "G0 X{}Y{}", x, y),
            GCodeCtrl::M3 { s } => writeln!(fmt, "M3 S{}", s),
            GCodeCtrl::M5 => writeln!(fmt, "M5"),
            GCodeCtrl::G1 { x, y, f } => writeln!(fmt, "G1 X{}Y{}F{}", x, y, f),
        }
    }
}
