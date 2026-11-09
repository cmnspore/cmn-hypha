//! Environment reads with thread-local overrides for unit tests.

#[cfg(test)]
std::thread_local! {
    static OVERRIDES: std::cell::RefCell<std::collections::BTreeMap<String, Option<std::ffi::OsString>>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

pub(crate) fn var(name: &str) -> Result<String, std::env::VarError> {
    #[cfg(test)]
    if let Some(value) = OVERRIDES.with(|overrides| overrides.borrow().get(name).cloned()) {
        return value
            .ok_or(std::env::VarError::NotPresent)
            .and_then(|value| value.into_string().map_err(std::env::VarError::NotUnicode));
    }
    std::env::var(name)
}

#[cfg(test)]
pub(crate) fn set_var(name: &str, value: impl AsRef<std::ffi::OsStr>) {
    OVERRIDES.with(|overrides| {
        overrides
            .borrow_mut()
            .insert(name.to_owned(), Some(value.as_ref().to_owned()));
    });
}

#[cfg(test)]
pub(crate) fn remove_var(name: &str) {
    OVERRIDES.with(|overrides| {
        overrides.borrow_mut().insert(name.to_owned(), None);
    });
}
