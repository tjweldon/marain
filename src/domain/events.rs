// use super::{app::Room, chat_log::MessageLog, notification_log::NotificationLog, user::User};

use super::{chat_log::MessageLog, notification_log::NotificationLog, room::Room, user::User};

#[derive(Clone)]
pub enum Event {
    UserRegistered {
        token: String,
    },
    UserJoined {
        user: User,
        room: Room,
        msg_log: Vec<MessageLog>,
        notifications: Vec<NotificationLog>,
        occupant_names: Vec<String>,
    },
    UserLeft {
        user: User,
        room: Room,
        msg_log: Vec<MessageLog>,
        notifications: Vec<NotificationLog>,
        occupant_names: Vec<String>,
    },
    MsgReceived {
        msg: MessageLog,
    },
    // Notify {
    //     notice: Vec<NotificationLog>,
    // }
}
