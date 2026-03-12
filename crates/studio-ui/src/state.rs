use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    /// Get the storage string for the theme mode.
    ///
    /// Converts the variant into the static string used for persistence: "light" or "dark".
    ///
    /// # Examples
    ///
    /// ```
    /// let s = ThemeMode::Dark.as_str();
    /// assert_eq!(s, "dark");
    /// ```
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    /// Convert a storage string into a ThemeMode.
    ///
    /// If `input` is exactly `"dark"`, this returns `ThemeMode::Dark`; for any other value it returns `ThemeMode::Light`.
    ///
    /// # Examples
    ///
    /// ```
    /// use crate::state::ThemeMode;
    ///
    /// assert_eq!(ThemeMode::from_storage_value("dark"), ThemeMode::Dark);
    /// assert_eq!(ThemeMode::from_storage_value("light"), ThemeMode::Light);
    /// assert_eq!(ThemeMode::from_storage_value("anything-else"), ThemeMode::Light);
    /// ```
    pub fn from_storage_value(input: &str) -> Self {
        match input {
            "dark" => ThemeMode::Dark,
            _ => ThemeMode::Light,
        }
    }
}

impl FromStr for ThemeMode {
    type Err = core::convert::Infallible;

    /// Parses a string into a `ThemeMode`.
    ///
    /// The input `"dark"` maps to `ThemeMode::Dark`; any other value maps to `ThemeMode::Light`.
    ///
    /// # Examples
    ///
    /// ```
    /// use crate::state::ThemeMode;
    /// use std::str::FromStr;
    ///
    /// assert_eq!(ThemeMode::from_str("dark").unwrap(), ThemeMode::Dark);
    /// assert_eq!(ThemeMode::from_str("light").unwrap(), ThemeMode::Light);
    /// assert_eq!(ThemeMode::from_str("unexpected").unwrap(), ThemeMode::Light);
    /// // Using `str::parse`
    /// assert_eq!("dark".parse::<ThemeMode>().unwrap(), ThemeMode::Dark);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_storage_value(s))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeStore {
    mode: ThemeMode,
}

impl ThemeStore {
    /// Creates a ThemeStore initialized with the provided theme mode.
    ///
    /// # Examples
    ///
    /// ```
    /// let store = ThemeStore::new(ThemeMode::Dark);
    /// assert_eq!(store.mode(), ThemeMode::Dark);
    /// ```
    pub fn new(initial: ThemeMode) -> Self {
        Self { mode: initial }
    }

    /// Gets the current theme mode.
    ///
    /// # Returns
    ///
    /// The current `ThemeMode`.
    ///
    /// # Examples
    ///
    /// ```
    /// let store = ThemeStore::new(ThemeMode::Dark);
    /// assert_eq!(store.mode(), ThemeMode::Dark);
    /// ```
    pub fn mode(&self) -> ThemeMode {
        self.mode
    }

    /// Sets the current theme mode to the provided value.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut store = ThemeStore::new(ThemeMode::Light);
    /// store.set_mode(ThemeMode::Dark);
    /// assert_eq!(store.mode(), ThemeMode::Dark);
    /// ```
    pub fn set_mode(&mut self, mode: ThemeMode) {
        self.mode = mode;
    }

    /// Toggles the current theme between `Light` and `Dark`.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut store = ThemeStore::new(ThemeMode::Light);
    /// store.toggle();
    /// assert_eq!(store.mode(), ThemeMode::Dark);
    /// store.toggle();
    /// assert_eq!(store.mode(), ThemeMode::Light);
    /// ```
    pub fn toggle(&mut self) {
        self.mode = match self.mode {
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        }
    }

    /// Get the current theme mode as a storage string.
    ///
    /// The returned string is "light" for Light and "dark" for Dark.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut store = ThemeStore::new(ThemeMode::Dark);
    /// assert_eq!(store.storage_value(), "dark".to_string());
    /// ```
    pub fn storage_value(&self) -> String {
        self.mode.as_str().to_string()
    }
}
