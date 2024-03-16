use std::collections::{HashMap, VecDeque};

use chrono::Utc;
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::StreamExt;
use marain_api::prelude::Timestamp;

use crate::domain::{chat_log::MessageLog, user::User};

use super::{
    commands::{Command, CommandPayload},
    events::Event,
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
    max_logs: usize,
}

impl AppState {
    fn new() -> Self {
        Self {
            occupancy: HashMap::from([(Room::from("Hub"), vec![])]),
            chat_logs: HashMap::from([(Room::from("Hub"), VecDeque::new())]),
            max_logs: 25,
        }
    }

    fn room_subscribers(&self, room: &Room) -> Vec<User> {
        self.occupancy.get(room).unwrap_or(&vec![]).clone()
    }

    fn add_user_to_room(&mut self, user: &User, room: &Room) {
        self.occupancy
            .entry(room.clone())
            .and_modify(|members| members.push(user.clone()))
            .or_insert(vec![user.clone()]);
    }

    fn remove_user_from_room(&mut self, target_room: &Room, user: &User) -> Result<()> {
        let occupants = self.occupancy.get_mut(target_room).unwrap();

        // idk which approach is better here.
        // self.occupancy
        //     .entry(target_room.clone())
        //     .and_modify(|members| members.retain(|occupant| occupant != user));

        if let Some(index) = occupants.iter().position(|occupant| *occupant == *user) {
            occupants.swap_remove(index);
            log::info!(
                "Removed User: {} from Room: {}",
                user.name,
                target_room.name
            );
            Ok(())
        } else {
            Err(anyhow!(
                "Could not find User: {} in Room: {}",
                user.name,
                target_room.name
            ))
        }
    }
}

struct CommandHandler {
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

        match command {
            Command {
                user,
                payload: CommandPayload::RegisterUser(event_sink),
            } => {
                event_buf.push_back(self.register_user(user.clone()));
                event_buf.push_back(self.insert_occupant(&user, &Room::from("Hub")));
                Ok(())
            }
            Command {
                user,
                payload: CommandPayload::MoveUser { target_room },
            } => {
                match self.remove_occupant(&user, &target_room) {
                    Ok(broadcast) => event_buf.push_back(broadcast),
                    Err(e) => log::error!("{e}"),
                }
                event_buf.push_back(self.insert_occupant(&user, &target_room));
                Ok(())
            }
            Command {
                user,
                payload:
                    CommandPayload::RecordMessage {
                        target_room,
                        message,
                    },
            } => {
                let logs = self.state.chat_logs.get_mut(&target_room).unwrap();
                let msg_log = MessageLog {
                    username: user.name,
                    timestamp: Utc::now(),
                    contents: message,
                };
                logs.push_back(msg_log.clone());
                if logs.len() < self.state.max_logs {
                    logs.pop_front();
                }
                let br = Broadcast::new(
                    Event::MsgReceived { msg: msg_log },
                    self.state.room_subscribers(&target_room),
                );
                event_buf.push_back(br);
                Ok(())
            }

            _ => Err(anyhow!("{command:?} not implemented in CommandHandler")),
        }
    }

    fn register_user(&mut self, user: User) -> Broadcast {
        Broadcast::new(
            Event::UserRegistered {
                token: user.id.clone(),
            },
            vec![user.clone()],
        )
    }

    fn remove_occupant(&mut self, user: &User, room: &Room) -> Result<Broadcast> {
        self.state.remove_user_from_room(room, user)?;
        Ok(Broadcast::new(
            Event::UserLeft {
                user: user.clone(),
                room: room.clone(),
            },
            self.state.room_subscribers(&room),
        ))
    }

    fn insert_occupant(&mut self, user: &User, room: &Room) -> Broadcast {
        self.state.add_user_to_room(user, &room);

        Broadcast::new(
            Event::UserJoined {
                user: user.clone(),
                room: room.clone(),
            },
            self.state.room_subscribers(&room),
        )
    }
}

struct App {
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

    pub fn new(
        gateway_source: UnboundedReceiver<Command>,
        command_handler: CommandHandler,
        event_bus: EventBus,
    ) -> Self {
        Self {
            gateway_source,
            command_handler,
            event_bus,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // event buffer to limit re-allocation?
        let mut event_buf: VecDeque<Broadcast> = VecDeque::new();
        while let Some(command) = self.gateway_source.next().await {
            match command.clone() {
                Command {
                    user,
                    payload: CommandPayload::RegisterUser(delivery_channel, ..),
                } => self.event_bus.subscribe(user, delivery_channel),
                Command {
                    user,
                    payload: CommandPayload::DropUser,
                } => self.event_bus.unsubscribe(user),
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
        }

        Ok(())
    }
}
