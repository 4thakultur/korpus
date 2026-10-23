//! Field-level validation errors shared by form handlers.

#[derive(Clone, Debug, Default)]
pub struct FieldErrors(Vec<(&'static str, String)>);

impl FieldErrors {
    pub fn add(&mut self, field: &'static str, msg: impl Into<String>) {
        self.0.push((field, msg.into()));
    }

    pub fn get(&self, field: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, m)| m.as_str())
    }

    pub fn has(&self, field: &str) -> bool {
        self.get(field).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(&'static str, String)> {
        self.0.iter()
    }
}

/// A simple labelled text input rendered in a loop by templates.
pub struct InputField {
    pub id: &'static str,
    pub label: &'static str,
    pub value: String,
    pub required: bool,
}
