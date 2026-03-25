use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::broadcast::Sender;
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

type RoomMap = Arc<RwLock<HashMap<String, broadcast::Sender<Message>>>>;

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

async fn process_client(
    mut reader: BufReader<OwnedReadHalf>,
    mut writer: OwnedWriteHalf,
    room_tx: Sender<Message>,
    id: Uuid,
) {
    let mut line = String::new();
    let mut room_rx = room_tx.subscribe();

    loop {
        line.clear();
        tokio::select! {
            // Client sent us something
            result = reader.read_line(&mut line) => {
                 // If empty line or error we want to get out
                match result {
                    Ok(0) | Err(_) => break,
                    Ok(n_bytes) => {
                        println!("Received {n_bytes} bytes from client: [{id}]");
                        // Build message
                        let msg = Message::new(id, line.clone());

                        // broadcasting to all clients connected to this room
                        let _ = room_tx.send(msg);
                    }
                }
            }
            // Room broadcast Received
            result = room_rx.recv() => {
                match result {
                    Ok(msg) => {
                        // We don't want to display our own messages
                        if id != msg.sender {
                            write_message(&msg.content, &mut writer).await;
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

        // Generate unique ID to identify client
        let id = get_uuid();

        println!("[{addr}] Connected !");
        let rooms = rooms.clone();

        tokio::spawn(async move {
            let (reader, mut writer) = socket.into_split();
            let reader = BufReader::new(reader);

            // retrieve room broadcast
            let room_tx = {
                let room_guard = rooms.read().await;
                get_or_create_room(&mut room_guard.to_owned(), "general")
            };

            write_message("Welcome! you're in #general channel\n", &mut writer).await;

            // processing client
            process_client(reader, writer, room_tx, id).await;

            println!("[{addr}] Disconnected");
        });
    }
}
