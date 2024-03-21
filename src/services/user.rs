#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub name: String,
    pub shared_secret: [u8; 32],
}

impl User {
    pub fn new(id: String, name: String, shared_secret: [u8; 32]) -> Self {
        User {
            id,
            name,
            shared_secret,
        }
    }
}
