use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

mod command;

type RoomMap = Arc<RwLock<HashMap<String, broadcast::Sender<Message>>>>;

struct ClientSession<'a> {
    id: Uuid,
    room_tx: Sender<Message>,
    room_rx: Receiver<Message>,
    reader: &'a mut BufReader<OwnedReadHalf>,
    writer: &'a mut OwnedWriteHalf,
    name: String,
    // TODO Maybe put in a shared context with other usefuls values
    rooms: RoomMap,
}

impl<'a> ClientSession<'a> {
    fn new(
        room_tx: Sender<Message>,
        reader: &'a mut BufReader<OwnedReadHalf>,
        writer: &'a mut OwnedWriteHalf,
        name: String,
        rooms: RoomMap,
    ) -> Self {
        Self {
            // Generate unique ID to identify client
            id: get_uuid(),
            // Subscribe to the room sender
            room_rx: room_tx.subscribe(),
            room_tx,
            reader,
            writer,
            name,
            rooms,
        }
    }

    // Refresh room broadcast receiver
    fn refresh(&mut self) {
        self.room_rx = self.room_tx.subscribe()
    }

    /// broadcast a message to the linked room
    fn broadcast_message(&self, msg: &str) {
        let msg = Message::new(self.id, msg.to_string(), self.name.clone());
        if let Err(e) = self.room_tx.send(msg) {
            eprintln!(
                "[ERROR] Error while broadcasting a message from client [{}] details: {e}",
                self.id
            );
        }
    }

    /// Send an error to the client
    async fn send_error(&mut self, error: &str) {
        if let Err(e) = self.writer.write_all(error.as_bytes()).await {
            eprintln!(
                "[ERROR] Error while writing an error message to client [{}] details: {e}",
                self.id
            );
        }
    }

    async fn join_room(&mut self, room_name: &str) {
        let room_tx = {
            let mut room_guard = self.rooms.write().await;
            get_or_create_room(&mut room_guard, room_name)
        };

        // Update current session
        self.room_tx = room_tx;
        self.refresh();
    }

    // TODO Maybe rework this function
    /// Return a vec of all rooms entries
    async fn list_rooms(&self) -> Vec<String> {
        self.rooms
            .read()
            .await
            .keys()
            .filter(|key| !key.is_empty())
            .cloned()
            .collect::<Vec<String>>()
    }

    fn disconnect(&self) {
        todo!()
    }

    fn rename(&mut self, name: String) {
        self.name = name;
    }

    async fn write_message(&mut self, msg: &str) {
        let msg = match msg.ends_with('\n') {
            true => msg.to_string(),
            false => format!("{}\n", msg),
        };

        if let Err(e) = self.writer.write_all(msg.as_bytes()).await {
            println!("An error occured while writing to client: [{e}]");
        }
    }
}

#[derive(Debug, Clone)]
struct Message {
    sender: Uuid,
    name: String,
    content: String,
}

impl Message {
    fn new(sender: Uuid, msg: String, name: String) -> Self {
        Self {
            sender,
            content: msg,
            name,
        }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}: {}", self.name, self.content)
    }
}

fn get_uuid() -> Uuid {
    Uuid::new_v4()
}

fn get_or_create_room(
    rooms: &mut HashMap<String, broadcast::Sender<Message>>,
    name: &str,
) -> broadcast::Sender<Message> {
    rooms
        .entry(name.to_string())
        .or_insert_with(|| {
            let (tx, _) = broadcast::channel(100); //message buffer
            tx
        })
        .clone()
}

async fn process_client<'a>(mut session: ClientSession<'a>) {
    let mut buf = Vec::<u8>::new();
    loop {
        buf.clear();
        tokio::select! {
            // Client sent us something reading until end of line
            result = session.reader.read_until(b'\n', &mut buf) => {
                // If empty line or error we want to get out
                match result {
                    Ok(0) | Err(_) => break,
                    Ok(n_bytes) => {
                        println!("Received {n_bytes} bytes from client: [{}]", session.id);

                        let line = String::from_utf8(buf.clone()).unwrap_or(String::from("Unable to parse Message to valid utf8"));

                        handle_client_input(&line, &mut session).await;
                    }
                }
            }
            // Room broadcast Received
            result = session.room_rx.recv() => {
                match result {
                    Ok(msg) => {
                        // We don't want to display our own messages
                        if session.id != msg.sender {
                            session.write_message(&msg.to_string()).await;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

/// Work in progress
async fn handle_client_input(input: &str, session: &'_ mut ClientSession<'_>) {
    let input = command::sanitize(input);

    // handle input as a command
    if command::is_a_command(&input) {
        match command::try_parse(&input) {
            Ok(cmd) => {
                // execute Command
                cmd.execute(session).await;
            }
            Err(e) => {
                println!(
                    "[WARNING]: error while handling client [{}] command: {}",
                    session.id, e
                );
                session.send_error(&e.to_string()).await;
            }
        };
    } else {
        // Handle input as a message
        session.broadcast_message(&input);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    // Initialize rooms map with the default general room
    let mut rooms: HashMap<String, broadcast::Sender<Message>> =
        HashMap::<String, broadcast::Sender<Message>>::new();
    get_or_create_room(&mut rooms, "general");

    // Making rooms thread_safe
    let rooms: RoomMap = Arc::new(RwLock::new(rooms));

    println!("Chat server is running on :8080");

    loop {
        let (socket, addr) = listener.accept().await?;

        println!("[{addr}] Connected !");
        let rooms = rooms.clone();

        tokio::spawn(async move {
            let (reader, mut writer) = socket.into_split();
            let mut reader = BufReader::new(reader);

            // retrieve room broadcast
            // TODO Setup this in a better way
            let room_tx = {
                let room_guard = rooms.read().await;
                get_or_create_room(&mut room_guard.to_owned(), "general")
            };

            // build user session
            // TODO Put a dynamic name
            let mut session = ClientSession::new(
                room_tx,
                &mut reader,
                &mut writer,
                "Guest".to_string(),
                rooms,
            );

            // Walcome the user
            session
                .write_message("Welcome! you're in #general channel")
                .await;

            // processing client
            process_client(session).await;

            println!("[{addr}] Disconnected");
        });
    }
}
