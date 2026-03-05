use protocol::{decode, encode, ClientMessage, ServerMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;

async fn start_server() -> (u16, CancellationToken) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = CancellationToken::new();
    let server_token = token.clone();
    tokio::spawn(server::run(listener, server_token));
    (port, token)
}

struct TestClient {
    lines: tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

impl TestClient {
    async fn connect(port: u16, username: &str) -> Self {
        let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let (reader, mut writer) = stream.into_split();

        let join = encode(&ClientMessage::Join {
            username: username.into(),
        });
        writer.write_all(join.as_bytes()).await.unwrap();

        Self {
            lines: BufReader::new(reader).lines(),
            writer,
        }
    }

    async fn send_msg(&mut self, content: &str) {
        let msg = encode(&ClientMessage::Send {
            content: content.into(),
        });
        self.writer.write_all(msg.as_bytes()).await.unwrap();
    }

    async fn leave(&mut self) {
        let msg = encode(&ClientMessage::Leave);
        self.writer.write_all(msg.as_bytes()).await.unwrap();
    }

    async fn recv(&mut self) -> ServerMessage {
        let line = timeout(Duration::from_secs(2), self.lines.next_line())
            .await
            .expect("timed out waiting for message")
            .unwrap()
            .expect("connection closed");
        decode(&line).unwrap()
    }

    async fn try_recv(&mut self) -> Option<ServerMessage> {
        match timeout(Duration::from_millis(100), self.lines.next_line()).await {
            Ok(Ok(Some(line))) => Some(decode(&line).unwrap()),
            _ => None,
        }
    }
}

#[tokio::test]
async fn user_join_is_broadcast() {
    let (port, token) = start_server().await;

    let mut alice = TestClient::connect(port, "alice").await;
    let _bob = TestClient::connect(port, "bob").await;

    let msg = alice.recv().await;
    assert_eq!(
        msg,
        ServerMessage::Joined {
            username: "bob".into()
        }
    );

    token.cancel();
}

#[tokio::test]
async fn message_is_broadcast_to_others() {
    let (port, token) = start_server().await;

    let mut alice = TestClient::connect(port, "alice").await;
    let mut bob = TestClient::connect(port, "bob").await;

    alice.recv().await; // bob joined

    bob.send_msg("hello from bob").await;

    let msg = alice.recv().await;
    assert_eq!(
        msg,
        ServerMessage::Message {
            username: "bob".into(),
            content: "hello from bob".into(),
        }
    );

    assert!(bob.try_recv().await.is_none());

    token.cancel();
}

#[tokio::test]
async fn leave_is_broadcast() {
    let (port, token) = start_server().await;

    let mut alice = TestClient::connect(port, "alice").await;
    let mut bob = TestClient::connect(port, "bob").await;

    alice.recv().await; // bob joined

    bob.leave().await;

    let msg = alice.recv().await;
    assert_eq!(
        msg,
        ServerMessage::Left {
            username: "bob".into()
        }
    );

    token.cancel();
}

#[tokio::test]
async fn duplicate_username_is_rejected() {
    let (port, token) = start_server().await;

    let _alice = TestClient::connect(port, "alice").await;
    let mut alice2 = TestClient::connect(port, "alice").await;

    let msg = alice2.recv().await;
    assert!(matches!(msg, ServerMessage::Error { .. }));

    token.cancel();
}

#[tokio::test]
async fn disconnect_triggers_leave_broadcast() {
    let (port, token) = start_server().await;

    let mut alice = TestClient::connect(port, "alice").await;
    let bob = TestClient::connect(port, "bob").await;

    alice.recv().await; // bob joined

    drop(bob);

    let msg = alice.recv().await;
    assert_eq!(
        msg,
        ServerMessage::Left {
            username: "bob".into()
        }
    );

    token.cancel();
}

#[tokio::test]
async fn multiple_users_receive_messages() {
    let (port, token) = start_server().await;

    let mut alice = TestClient::connect(port, "alice").await;
    let mut bob = TestClient::connect(port, "bob").await;
    let mut charlie = TestClient::connect(port, "charlie").await;

    // Drain join notifications
    alice.recv().await; // bob joined
    alice.recv().await; // charlie joined
    bob.recv().await; // charlie joined

    alice.send_msg("hey everyone").await;

    let bob_msg = bob.recv().await;
    let charlie_msg = charlie.recv().await;

    assert_eq!(
        bob_msg,
        ServerMessage::Message {
            username: "alice".into(),
            content: "hey everyone".into(),
        }
    );
    assert_eq!(bob_msg, charlie_msg);

    assert!(alice.try_recv().await.is_none());

    token.cancel();
}

#[tokio::test]
async fn empty_username_is_rejected() {
    let (port, token) = start_server().await;

    let mut client = TestClient::connect(port, "  ").await;

    let msg = client.recv().await;
    assert!(matches!(msg, ServerMessage::Error { .. }));

    token.cancel();
}

#[tokio::test]
async fn username_too_long_is_rejected() {
    let (port, token) = start_server().await;

    let long_name = "a".repeat(100);
    let mut client = TestClient::connect(port, &long_name).await;

    let msg = client.recv().await;
    assert!(matches!(msg, ServerMessage::Error { .. }));

    token.cancel();
}
