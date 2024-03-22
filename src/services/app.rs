use std::collections::{HashMap, VecDeque};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::StreamExt;

use super::{
    chat_log::MessageLog,
    commands::{Command, CommandPayload},
    events::Event,
    notification_log::NotificationLog,
    user::User,
};

use anyhow::{anyhow, Result};

struct EventBus {
    subscribers: HashMap<User, UnboundedSender<Event>>,
}

impl EventBus {
    fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }

    pub fn publish(&mut self, broadcast: &Broadcast) {
        for user in &broadcast.subscribers {
            if let Some(channel) = self.subscribers.get(user) {
                channel.unbounded_send(broadcast.event.clone()).unwrap();
            }
        }
    }

    pub fn subscribe(
        &mut self,
        user: User,
        delivery_channel: UnboundedSender<Event>,
    ) -> Result<()> {
        match self.subscribers.insert(user, delivery_channel) {
            Some(_) => Err(anyhow!("We got a double subscription chief")),
            None => Ok(()),
        }
    }

    pub fn unsubscribe(&mut self, user: User) -> Result<()> {
        match self.subscribers.remove(&user) {
            Some(_) => Ok(()),
            None => Err(anyhow!(
                "Tried to unsubscribe a user that was not subscribed."
            )),
        }
    }
}

struct Broadcast {
    event: Event,
    subscribers: Vec<User>,
}

impl Broadcast {
    fn new(event: Event, subscribers: Vec<User>) -> Self {
        Self { event, subscribers }
    }
}

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

struct AppState {
    occupancy: HashMap<Room, Vec<User>>,
    chat_logs: HashMap<Room, VecDeque<MessageLog>>,
    notifications: HashMap<Room, VecDeque<NotificationLog>>,
    max_logs: usize,
}

impl AppState {
    fn new() -> Self {
        Self {
            occupancy: HashMap::from([(Room::default(), vec![])]),
            chat_logs: HashMap::from([(Room::default(), VecDeque::new())]),
            notifications: HashMap::from([(Room::default(), VecDeque::new())]),
            max_logs: 25,
        }
    }

    fn room_subscribers(&self, room: &Room) -> Vec<User> {
        self.occupancy.get(room).unwrap_or(&vec![]).clone()
    }

    fn room_chat_logs(&self, room: &Room) -> Vec<MessageLog> {
        self.chat_logs
            .get(&room)
            .unwrap_or(&VecDeque::new())
            .iter()
            .map(|msg| msg.clone())
            .collect()
    }

    fn room_notifications(&self, room: &Room) -> Vec<NotificationLog> {
        self.notifications
            .get(&room)
            .unwrap_or(&VecDeque::new())
            .iter()
            .map(|msg| msg.clone())
            .collect()
    }

    fn occupant_names(&self, room: &Room) -> Vec<String> {
        self.room_subscribers(&room)
            .iter()
            .map(|sub| sub.name.clone())
            .collect()
    }

    fn add_user_to_room(&mut self, user: &User, room: &Room) {
        self.occupancy
            .entry(room.clone())
            .and_modify(|members| members.push(user.clone()))
            .or_insert(vec![user.clone()]);
    }

    fn get_occupied_room(&self, user: &User) -> Option<Room> {
        for (room, occupants) in &self.occupancy {
            if occupants.contains(&user) {
                return Some(room.clone());
            }
        }
        return None;
    }

    fn remove_user_from_room(&mut self, user: &User, notice: NotificationLog) {
        let Some(room) = self.get_occupied_room(user) else {
            log::warn!(
                "Could not find user {user:?} in any room occpancy list when trying to remove"
            );
            return;
        };

        let Some(occupants) = self.occupancy.get_mut(&room) else {
            log::warn!("Could not find occupants associated to a room when trying to remove.");
            return;
        };

        let Some(index) = occupants.iter().position(|occupant| *occupant == *user) else {
            log::warn!("Could not find index of user {user:?} in occupants when trying to remove.");
            return;
        };

        occupants.swap_remove(index);
        self.record_notification(user, notice);
    }

    fn record_chat_message(&mut self, user: &User, msg: MessageLog) -> &[User] {
        for (room, occupants) in &self.occupancy {
            if occupants.contains(user) {
                // log::info!("{}", room.name);
                self.chat_logs
                    .entry(room.clone())
                    .and_modify(|logs| {
                        logs.push_back(msg.clone());
                        if logs.len() > self.max_logs {
                            logs.pop_front();
                        }
                    })
                    .or_insert(vec![msg].into());
                return occupants;
            }
        }

        return &[];
    }

    fn record_notification(&mut self, user: &User, notice: NotificationLog) {
        for (room, occupants) in &self.occupancy {
            if occupants.contains(user) {
                self.notifications
                    .entry(room.clone())
                    .and_modify(|logs| {
                        logs.push_back(notice.clone());
                        if logs.len() > self.max_logs {
                            logs.pop_front();
                        }
                    })
                    .or_insert(vec![notice.clone()].into());
            }
        }
    }
}

pub struct CommandHandler {
    state: AppState,
}

impl CommandHandler {
    fn new(state: AppState) -> Self {
        Self { state }
    }

    fn handle(&mut self, command: Command, event_buf: &mut VecDeque<Broadcast>) -> Result<()> {
        // Changed return from Result<Broadcast> -> Result<Vec<Broadcast>> -> Result<()>.
        // This is because some Commands may produce multiple broadcasts,
        // eg. MoveRoom should produce a UserLeft & UserJoined Broadcast for each event.
        // Was creating an empty Vec<Broadcast> here but it seems wasteful,
        // Decided to pass buffer instead?
        // Still returning Result<()> for fault tolerance around publishing

        let user = command.user.clone();

        match command.payload.clone() {
            CommandPayload::DropUser => {
                self.handle_drop_user(&user, event_buf);
                Ok(())
            }

            CommandPayload::RegisterUser(..) => {
                event_buf.push_back(self.register_user(user.clone()));
                event_buf.push_back(self.insert_occupant(&user, &Room::from("Hub")));
                Ok(())
            }

            CommandPayload::MoveUser { target_room } => {
                match self.remove_occupant(&user) {
                    Some(broadcast) => {
                        event_buf.push_back(broadcast);
                    }
                    None => {
                        log::error!("Failed to remove occupant: {user:?} in response to command.")
                    }
                }
                event_buf.push_back(self.insert_occupant(&user, &target_room));
                Ok(())
            }
            CommandPayload::RecordMessage { message } => {
                let msg_log = MessageLog::from_user(&user, message);
                let recipients: Vec<User> =
                    Vec::from(self.state.record_chat_message(&user, msg_log.clone()));

                let br = Broadcast::new(Event::MsgReceived { msg: msg_log }, recipients);
                event_buf.push_back(br);
                Ok(())
            }
            _ => Err(anyhow!("{:?} not implemented in CommandHandler", command)),
        }
    }

    fn handle_drop_user(&mut self, user: &User, event_buf: &mut VecDeque<Broadcast>) {
        let room = self
            .state
            .get_occupied_room(&user)
            .unwrap_or(Room::default());
        let mut subscribers = self.state.room_subscribers(&room);
        if !subscribers.contains(&user) {
            subscribers.push(user.clone());
        }

        let broadcast = self.remove_occupant(&user).unwrap_or(Broadcast {
            event: Event::UserLeft {
                user: user.clone(),
                room: room.clone(),
                msg_log: vec![],
                notifications: vec![],
                occupant_names: self.state.occupant_names(&room),
            },
            subscribers,
        });
        event_buf.push_back(broadcast);
    }

    fn register_user(&mut self, user: User) -> Broadcast {
        Broadcast::new(
            Event::UserRegistered {
                token: user.id.clone(),
            },
            vec![user.clone()],
        )
    }

    fn remove_occupant(&mut self, user: &User) -> Option<Broadcast> {
        let Some(current_room) = self.state.get_occupied_room(user) else {
            return None;
        };
        let notice = NotificationLog::new(format!("{} left {}", user.name, current_room.name));

        self.state.remove_user_from_room(user, notice);
        Some(Broadcast::new(
            Event::UserLeft {
                user: user.clone(),
                room: current_room.clone(),
                occupant_names: self.state.occupant_names(&current_room),
                notifications: self.state.room_notifications(&current_room),
                msg_log: self.state.room_chat_logs(&current_room),
            },
            self.state.room_subscribers(&current_room),
        ))
    }

    fn insert_occupant(&mut self, user: &User, room: &Room) -> Broadcast {
        self.state.add_user_to_room(user, &room);
        self.state.record_notification(
            user,
            NotificationLog::new(format!("{} joined {}", user.name, room.name)),
        );
        Broadcast::new(
            Event::UserJoined {
                user: user.clone(),
                room: room.clone(),
                msg_log: self.state.room_chat_logs(room),
                notifications: self.state.room_notifications(room),
                occupant_names: self.state.occupant_names(room),
            },
            self.state.room_subscribers(&room),
        )
    }
}

pub struct App {
    gateway_source: UnboundedReceiver<Command>,
    command_handler: CommandHandler,
    event_bus: EventBus,
}

impl App {
    pub fn init(command_source: UnboundedReceiver<Command>) -> Self {
        Self {
            gateway_source: command_source,
            command_handler: CommandHandler::new(AppState::new()),
            event_bus: EventBus::new(),
        }
    }

    pub fn run(mut self) {
        tokio::spawn(async move {
            match self.work().await {
                Err(e) => panic!("App exited unexpectedly with error {e}"),
                _ => (),
            }
        });
    }

    pub async fn work(&mut self) -> Result<()> {
        let mut event_buf: VecDeque<Broadcast> = VecDeque::new();
        let mut defer_unsubscribe: Option<User> = None;

        while let Some(command) = self.gateway_source.next().await {
            match command.clone() {
                Command {
                    user,
                    payload: CommandPayload::RegisterUser(delivery_channel, ..),
                } => self.event_bus.subscribe(user, delivery_channel),
                Command {
                    user,
                    payload: CommandPayload::DropUser,
                } => {
                    defer_unsubscribe = Some(user.clone());
                    Ok(())
                }
                _ => Ok(()),
            }?;
            match self.command_handler.handle(command, &mut event_buf) {
                Ok(_) => {
                    while let Some(cast) = event_buf.pop_front() {
                        self.event_bus.publish(&cast);
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
            if let Some(ref user) = defer_unsubscribe {
                match self.event_bus.unsubscribe(user.clone()) {
                    Err(e) => panic!("Failed to unsubscribe a user: {user:?} with Error: {e}"),
                    _ => {}
                };
                defer_unsubscribe = None;
            }
        }

        Ok(())
    }
}
