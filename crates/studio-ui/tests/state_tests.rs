use openstack_studio_ui::{ThemeMode, ThemeStore};

#[test]
fn theme_mode_roundtrip_string() {
    assert_eq!(ThemeMode::from_str("light"), ThemeMode::Light);
    assert_eq!(ThemeMode::from_str("dark"), ThemeMode::Dark);
    assert_eq!(ThemeMode::from_str("unknown"), ThemeMode::Light);
    assert_eq!(ThemeMode::Light.as_str(), "light");
    assert_eq!(ThemeMode::Dark.as_str(), "dark");
}

#[test]
fn theme_store_toggle_and_storage_value() {
    let mut store = ThemeStore::new(ThemeMode::Light);
    assert_eq!(store.mode(), ThemeMode::Light);
    assert_eq!(store.storage_value(), "light");

    store.toggle();
    assert_eq!(store.mode(), ThemeMode::Dark);
    assert_eq!(store.storage_value(), "dark");

    store.set_mode(ThemeMode::Light);
    assert_eq!(store.mode(), ThemeMode::Light);
}
