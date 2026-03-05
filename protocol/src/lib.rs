use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Join { username: String },
    Leave,
    Send { content: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Joined { username: String },
    Left { username: String },
    Message { username: String, content: String },
    Error { reason: String },
}

pub fn encode(msg: &impl Serialize) -> String {
    let mut s = serde_json::to_string(msg).expect("serialization should not fail");
    s.push('\n');
    s
}

pub fn decode<T: for<'de> Deserialize<'de>>(line: &str) -> serde_json::Result<T> {
    serde_json::from_str(line.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_message_round_trip() {
        let cases = vec![
            ClientMessage::Join {
                username: "alice".into(),
            },
            ClientMessage::Leave,
            ClientMessage::Send {
                content: "hello world".into(),
            },
        ];

        for msg in cases {
            let encoded = encode(&msg);
            let decoded: ClientMessage = decode(&encoded).unwrap();
            assert_eq!(msg, decoded);
        }
    }

    #[test]
    fn server_message_round_trip() {
        let cases = vec![
            ServerMessage::Joined {
                username: "bob".into(),
            },
            ServerMessage::Left {
                username: "bob".into(),
            },
            ServerMessage::Message {
                username: "bob".into(),
                content: "hey".into(),
            },
            ServerMessage::Error {
                reason: "username taken".into(),
            },
        ];

        for msg in cases {
            let encoded = encode(&msg);
            let decoded: ServerMessage = decode(&encoded).unwrap();
            assert_eq!(msg, decoded);
        }
    }

    #[test]
    fn decode_invalid_json_is_err() {
        let result: serde_json::Result<ClientMessage> = decode("not json");
        assert!(result.is_err());
    }

    #[test]
    fn encode_produces_newline_terminated_json() {
        let msg = ClientMessage::Leave;
        let encoded = encode(&msg);
        assert!(encoded.ends_with('\n'));
        assert_eq!(encoded.matches('\n').count(), 1);
    }
}
