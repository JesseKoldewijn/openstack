use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn from_storage_value(input: &str) -> Self {
        match input {
            "dark" => ThemeMode::Dark,
            _ => ThemeMode::Light,
        }
    }
}

impl FromStr for ThemeMode {
    type Err = core::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_storage_value(s))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeStore {
    mode: ThemeMode,
}

impl ThemeStore {
    pub fn new(initial: ThemeMode) -> Self {
        Self { mode: initial }
    }

    pub fn mode(&self) -> ThemeMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: ThemeMode) {
        self.mode = mode;
    }

    pub fn toggle(&mut self) {
        self.mode = match self.mode {
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        }
    }

    pub fn storage_value(&self) -> String {
        self.mode.as_str().to_string()
    }
}
