/*
 * @author t0bx
 * @since 23.10.2026
 */

use ratatui::style::{Color, Modifier, Style};
use relix_config::ColorSpec;
use relix_config::theme::{BorderKind, Theme, ThemePalette};
use relix_core::Environment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    Indexed256,
}

impl ColorDepth {
    /// Truecolor when the terminal advertises it via `COLORTERM`, otherwise the
    /// 256-color palette that virtually every terminal supports.
    pub fn detect() -> Self {
        let colorterm = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") || cfg!(windows) {
            Self::TrueColor
        } else {
            Self::Indexed256
        }
    }
}

pub fn convert(spec: ColorSpec, depth: ColorDepth) -> Color {
    match spec {
        ColorSpec::Reset => Color::Reset,
        ColorSpec::Indexed(index) => Color::Indexed(index),
        ColorSpec::Named(index) => named(index),
        ColorSpec::Rgb(red, green, blue) => match depth {
            ColorDepth::TrueColor => Color::Rgb(red, green, blue),
            ColorDepth::Indexed256 => Color::Indexed(nearest_xterm(red, green, blue)),
        },
    }
}

fn named(index: u8) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        _ => Color::White,
    }
}

/// Maps an RGB color to the closest entry of the xterm 256-color cube or grey ramp.
pub fn nearest_xterm(red: u8, green: u8, blue: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |value: u8| {
        LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, level)| (i32::from(**level) - i32::from(value)).abs())
            .map_or(0, |(index, _)| index as u8)
    };
    let (r, g, b) = (level(red), level(green), level(blue));
    let cube_index = 16 + 36 * r + 6 * g + b;
    let cube_color = (LEVELS[r as usize], LEVELS[g as usize], LEVELS[b as usize]);
    let average = (u16::from(red) + u16::from(green) + u16::from(blue)) / 3;
    let grey_step = ((average.saturating_sub(8)) / 10).min(23) as u8;
    let grey_value = 8 + 10 * grey_step;
    let distance = |(r2, g2, b2): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(red, r2) + d(green, g2) + d(blue, b2)
    };
    if distance((grey_value, grey_value, grey_value)) < distance(cube_color) {
        232 + grey_step
    } else {
        cube_index
    }
}

pub type Palette = ThemePalette<Color>;

/// Resolved ratatui colors and style helpers for the active theme.
#[derive(Debug, Clone)]
pub struct Styles {
    pub colors: Palette,
    pub borders: BorderKind,
}

impl std::ops::Deref for Styles {
    type Target = Palette;

    fn deref(&self) -> &Palette {
        &self.colors
    }
}

impl Styles {
    pub fn from_theme(theme: &Theme, depth: ColorDepth) -> Self {
        Self {
            colors: theme.colors.map(|spec| convert(*spec, depth)),
            borders: theme.borders,
        }
    }

    pub fn base(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }

    pub fn panel(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.surface)
    }

    pub fn muted(&self) -> Style {
        Style::default().fg(self.muted)
    }

    pub fn title(&self, focused: bool) -> Style {
        let color = if focused { self.accent } else { self.muted };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    pub fn border(&self, focused: bool) -> Style {
        Style::default().fg(if focused {
            self.border_focused
        } else {
            self.border
        })
    }

    pub fn selected(&self) -> Style {
        Style::default().fg(self.selection_fg).bg(self.selection_bg)
    }

    pub fn environment(&self, environment: Environment) -> Option<Color> {
        match environment {
            Environment::None => None,
            Environment::Development => Some(self.env_development),
            Environment::Testing => Some(self.env_testing),
            Environment::Staging => Some(self.env_staging),
            Environment::Production => Some(self.env_production),
        }
    }

    pub fn border_type(&self) -> ratatui::widgets::BorderType {
        use ratatui::widgets::BorderType;
        match self.borders {
            BorderKind::Plain => BorderType::Plain,
            BorderKind::Rounded => BorderType::Rounded,
            BorderKind::Double => BorderType::Double,
            BorderKind::Thick => BorderType::Thick,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_falls_back_to_xterm_palette() {
        assert_eq!(nearest_xterm(0, 0, 0), 16);
        assert_eq!(nearest_xterm(255, 255, 255), 231);
        assert_eq!(nearest_xterm(128, 128, 128), 244);
        assert_eq!(
            convert(ColorSpec::Rgb(255, 0, 0), ColorDepth::Indexed256),
            Color::Indexed(196)
        );
        assert_eq!(
            convert(ColorSpec::Rgb(1, 2, 3), ColorDepth::TrueColor),
            Color::Rgb(1, 2, 3)
        );
        assert_eq!(
            convert(ColorSpec::Named(9), ColorDepth::Indexed256),
            Color::LightRed
        );
    }
}
