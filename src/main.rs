use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

type RoomMap = Arc<RwLock<HashMap<String, broadcast::Sender<Message>>>>;

struct ClientSession<'a> {
    id: Uuid,
    room_tx: &'a Sender<Message>,
    room_rx: Receiver<Message>,
    reader: &'a mut BufReader<OwnedReadHalf>,
    writer: &'a mut OwnedWriteHalf,
}

impl<'a> ClientSession<'a> {
    fn new(
        room_tx: &'a Sender<Message>,
        reader: &'a mut BufReader<OwnedReadHalf>,
        writer: &'a mut OwnedWriteHalf,
    ) -> Self {
        Self {
            // Generate unique ID to identify client
            id: get_uuid(),
            // Subscribe to the room sender
            room_rx: room_tx.subscribe(),
            room_tx,
            reader,
            writer,
        }
    }

    // Change session broadcast sender to corresponding rooom and refresh room receiver
    fn change_room(&mut self, room_tx: &'a Sender<Message>) {
        self.room_tx = room_tx;
        self.refresh();
    }

    // Refresh room broadcast receiver
    fn refresh(&mut self) {
        self.room_rx = self.room_tx.subscribe()
    }
}

#[derive(Debug, Clone)]
struct Message {
    sender: Uuid,
    content: String,
}

impl Message {
    fn new(sender: Uuid, msg: String) -> Self {
        Self {
            sender,
            content: msg,
        }
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
                        // Build message
                        let msg = Message::new(session.id, String::from_utf8(buf.clone()).unwrap_or(String::from("Unable to parse Message to valid utf8")));

                        // broadcasting to all clients connected to this room
                        let _ = session.room_tx.send(msg);
                    }
                }
            }
            // Room broadcast Received
            result = session.room_rx.recv() => {
                match result {
                    Ok(msg) => {
                        // We don't want to display our own messages
                        if session.id != msg.sender {
                            write_message(&msg.content, session.writer).await;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

async fn write_message(msg: &str, writer: &mut OwnedWriteHalf) {
    if let Err(e) = writer.write_all(msg.as_bytes()).await {
        println!("An error occured while writing to client: [{e}]");
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
            let room_tx = {
                let room_guard = rooms.read().await;
                get_or_create_room(&mut room_guard.to_owned(), "general")
            };

            write_message("Welcome! you're in #general channel\n", &mut writer).await;

            // build user session
            let session = ClientSession::new(&room_tx, &mut reader, &mut writer);

            // processing client
            process_client(session).await;

            println!("[{addr}] Disconnected");
        });
    }
}
