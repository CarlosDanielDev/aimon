//! Looks taken from trade-ai: `neon` (indigo ground, neon inks) and `matrix` (phosphor green,
//! double rules). Colors by role, always paired with a glyph; `NO_COLOR` drops the colors.
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub matrix: bool,
    pub mono: bool,
    pub ok: Color,
    pub warn: Color,
    pub bad: Color,
    pub accent: Color,
    pub muted: Color,
    pub text: Color,
    pub brand: Color,
    pub value: Color,
    pub ground: Color,
}

impl Theme {
    fn neon(mono: bool) -> Self {
        Self {
            matrix: false,
            mono,
            ok: Color::Rgb(0x39, 0xff, 0x14),
            warn: Color::Rgb(0xff, 0xb0, 0x00),
            bad: Color::Rgb(0xff, 0x38, 0x60),
            accent: Color::Rgb(0x00, 0xe5, 0xff),
            muted: Color::Rgb(0x8a, 0x98, 0xc4),
            text: Color::Rgb(0xc8, 0xd3, 0xf5),
            brand: Color::Rgb(0xff, 0x2e, 0x97),
            value: Color::Rgb(0xb4, 0x8c, 0xff),
            ground: Color::Rgb(0x0b, 0x0e, 0x1a),
        }
    }

    fn matrix(mono: bool) -> Self {
        Self {
            matrix: true,
            mono,
            ok: Color::Rgb(0xa8, 0xff, 0x60),
            warn: Color::Rgb(0xff, 0xb0, 0x00),
            bad: Color::Rgb(0xff, 0x50, 0x50),
            accent: Color::Rgb(0xa8, 0xff, 0x60),
            muted: Color::Rgb(0x5f, 0x9a, 0x3c),
            text: Color::Rgb(0x8f, 0xe0, 0x50),
            brand: Color::Rgb(0xc8, 0xff, 0x90),
            value: Color::Rgb(0xd8, 0xff, 0xa8),
            ground: Color::Rgb(0, 0, 0),
        }
    }

    /// `NO_COLOR` set and non-empty means no color.
    pub fn detect() -> Self {
        Self::neon(std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()))
    }

    pub fn switched(self) -> Self {
        if self.matrix {
            Self::neon(self.mono)
        } else {
            Self::matrix(self.mono)
        }
    }

    pub fn fg(&self, c: Color) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().fg(c)
        }
    }

    pub fn bold(&self, c: Color) -> Style {
        self.fg(c).add_modifier(Modifier::BOLD)
    }

    pub fn muted(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            self.fg(self.muted)
        }
    }

    pub fn ground(&self) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().bg(self.ground).fg(self.text)
        }
    }

    pub fn border(&self) -> BorderType {
        if self.matrix {
            BorderType::Double
        } else {
            BorderType::Plain
        }
    }

    /// Filled / empty cells of a bar.
    pub fn bar(&self) -> (char, char) {
        if self.matrix {
            ('▓', '░')
        } else {
            ('━', '─')
        }
    }
}
