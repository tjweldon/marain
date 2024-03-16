#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct User {
    pub room: u64,
    pub id: String,
    pub up_to_date: bool,
    pub name: String,
    pub shared_secret: [u8; 32],
}

impl User {
    pub fn new(
        room: u64,
        id: String,
        up_to_date: bool,
        name: String,
        shared_secret: [u8; 32],
    ) -> Self {
        User {
            room,
            id,
            up_to_date,
            name,
            shared_secret,
        }
    }
}
