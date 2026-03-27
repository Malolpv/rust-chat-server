use thiserror::Error;

use crate::ClientSession;

#[derive(Error, Debug, PartialEq)]
pub enum CommmandParsingError {
    #[error("Invalid Command: {0}")]
    InvalidCommand(String),
    #[error("Unknown Command: {0}")]
    UnknownCommand(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    /// /join <room>    Leave current room, join new room, announce in both
    Join(String),
    /// /rename <name>    Change display name
    Rename(String),
    // /rooms    List all active rooms and member counts
    Rooms,
    /// /quit    Disconnect gracefully
    Quit,
}

impl Command {
    fn usage(cmd: Self) -> String {
        match cmd {
            Self::Join(_) => "Usage: /join <my_room> (argument cannot contains any whitespaces)\n",
            Self::Rooms => "Usage: /rooms",
            Self::Quit => "usage: /quit",
            Self::Rename(_) => {
                "Usage: /rename <my_name> (argument cannot contains any whitespaces\n"
            }
        }
        .to_string()
    }

    pub async fn execute(self, session: &mut ClientSession<'_>) {
        match self {
            Self::Join(room) => {
                println!("Client [{}] is trying to join room: {room}", session.id);
                session.join_room(&room).await;
                let msg = format!("Joined room #{}", room);
                session.write_message(&msg).await;
            }
            Self::Rename(name) => {
                println!("Client [{}] is trying to rename as: {name}", session.id);
                session.rename(name);
            }
            Self::Rooms => {
                println!("Client [{}] is listing rooms", session.id);
                session.list_rooms().await;
            }
            Self::Quit => {
                println!("Client [{}] is closing connection", session.id);
                session.disconnect().await;
            }
        }
    }
}

/// Sanitize input string to make sure there isnt any leading/trailing whitespaces or newlines
pub fn sanitize(input: &str) -> String {
    input
        .trim_matches(|c| char::is_whitespace(c) || c == '\n')
        .to_string()
}

pub fn is_a_command(input: &str) -> bool {
    input.starts_with('/')
}

/// Should receive a sanitized input and try to parse it into a command
/// Result can be :
/// - Valid command
/// - ErrorInvalidCommand
/// - ErrorUnknownCommand
pub fn try_parse(input: &str) -> Result<Command, CommmandParsingError> {
    // input should be clean at this point
    let input = input[1..]
        .split_once(char::is_whitespace)
        .unwrap_or((&input[1..], "")); //no arguments provided could be /quit or /room

    match input {
        ("join", args) => {
            let cmd = Command::Join(args.to_string());
            // Validation
            if args.contains(char::is_whitespace) {
                return Err(CommmandParsingError::InvalidCommand(Command::usage(cmd)));
            }
            Ok(Command::Join(args.to_string()))
        }
        ("rooms", _) => Ok(Command::Rooms),
        ("rename", args) => {
            let cmd = Command::Rename(args.to_string());
            // validation
            if args.contains(char::is_whitespace) {
                return Err(CommmandParsingError::InvalidCommand(Command::usage(cmd)));
            }
            Ok(cmd)
        }
        ("quit", _) => Ok(Command::Quit),
        (cmd, args) => Err(CommmandParsingError::UnknownCommand(format!(
            "/{} {}",
            cmd, args
        ))),
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn test_sanitize() {
        let mut input = String::from(" /MyCommand is not sanitized \n");

        let result = super::sanitize(&mut input);

        assert_eq!("/MyCommand is not sanitized".to_string(), result);
    }

    #[test]
    fn test_is_a_command() {
        let mut input = String::from(" \n/join room ");
        input = sanitize(&input);

        assert!(is_a_command(&input));
    }

    #[test]
    fn test_parse() {
        let mut input = String::from(" \n/join room ");
        input = sanitize(&input);

        let result = try_parse(&input);

        assert_eq!(Command::Join("room".to_string()), result.unwrap());
    }

    #[test]
    fn test_parse_with_2_arguments() {
        let mut input = String::from(" \n/join room");
        input = sanitize(&input);

        let result = try_parse(&input);

        assert_eq!(result.ok(), Some(Command::Join("room".to_string())));
    }

    #[test]
    fn test_parse_invalid_join() {
        let mut input = String::from(" \n/join invalid room");
        input = sanitize(&input);

        let result = try_parse(&input);

        assert_eq!(
            Some(CommmandParsingError::InvalidCommand(Command::usage(
                Command::Join("invalid room".to_string())
            ))),
            result.err()
        );
    }

    #[test]
    fn test_parse_quit_simple() {
        let input = sanitize("/quit");
        let result = try_parse(&input);
        assert_eq!(result.unwrap(), Command::Quit);
    }

    #[test]
    fn test_parse_rooms_simple() {
        let input = sanitize("/rooms");
        let result = try_parse(&input);
        assert_eq!(result.unwrap(), Command::Rooms);
    }

    #[test]
    fn test_parse_rename_valid() {
        let input = sanitize("/rename SuperUser");
        let result = try_parse(&input);
        assert_eq!(result.unwrap(), Command::Rename("SuperUser".to_string()));
    }

    #[test]
    fn test_parse_rename_with_spaces_fails() {
        let input = sanitize("/rename John Doe");
        let result = try_parse(&input);

        let expected_error = CommmandParsingError::InvalidCommand(Command::usage(Command::Rename(
            "John Doe".to_string(),
        )));
        assert_eq!(result.err().unwrap(), expected_error);
    }

    #[test]
    fn test_parse_unknown_command() {
        let input = sanitize("/my_unknown_command");
        let result = try_parse(&input);

        match result {
            Err(CommmandParsingError::UnknownCommand(msg)) => {
                assert!(msg.contains("/my_unknown_command"));
            }
            _ => panic!("Should return UnknownCommand"),
        }
    }

    #[test]
    fn test_parse_empty_command() {
        let input = sanitize("/");
        let result = try_parse(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_command_with_leading_whitespace_after_sanitize() {
        let raw_input = "   /join Lobby   ";
        let clean_input = sanitize(raw_input);
        let result = try_parse(&clean_input);

        assert_eq!(result.unwrap(), Command::Join("Lobby".to_string()));
    }

    #[test]
    fn test_is_a_command_negative() {
        assert!(!is_a_command("Not a command"));
        assert!(!is_a_command(" /whitespace should not be before slash"));
    }

    #[test]
    fn test_usage_messages() {
        let quit_usage = Command::usage(Command::Quit);
        assert!(quit_usage.to_lowercase().contains("usage"));
        assert!(quit_usage.contains("/quit"));

        let join_usage = Command::usage(Command::Join("".to_string()));
        assert!(join_usage.contains("/join <my_room>"));
    }
}
