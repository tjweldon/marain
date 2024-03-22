#[derive(Debug, Hash, Clone, PartialEq, Eq)]
pub struct Room {
    pub name: String,
}

impl Default for Room {
    fn default() -> Self {
        Room::from("Hub")
    }
}

impl From<&str> for Room {
    fn from(value: &str) -> Self {
        Self { name: value.into() }
    }
}
