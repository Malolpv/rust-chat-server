# Rust TCP Chat Server (WIP)

This project is a simple implementation of an async TCP based chat server in Rust. 


# Run project : 
To run this project make sure that rust and cargo are installed on your device.


Run this command to start the server on localhost:8080

```
cargo run --release
```


## Connect a client
Since this chat server is based on TCP you can use telnet to connect 

> telnet localhost 8080

## Available commands (WIP)
- /join <my_room>   Leave current room, join new room, announce in both
- /rename <name>    Change display name
- /rooms            List all available rooms
- /quit             Disconnect gracefully


