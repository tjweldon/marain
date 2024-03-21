use super::{app::Room, chat_log::MessageLog, user::User};

#[derive(Clone)]
pub enum Event {
    UserRegistered {
        token: String,
    },
    UserJoined {
        user: User,
        room: Room,
        msg_log: Vec<MessageLog>,
        occupant_names: Vec<String>,
    },
    UserLeft {
        user: User,
        room: Room,
    },
    MsgReceived {
        msg: MessageLog,
    },
}
