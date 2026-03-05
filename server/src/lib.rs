use std::sync::Arc;

use dashmap::DashMap;
use protocol::{decode, encode, ClientMessage, ServerMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const MAX_USERNAME_LEN: usize = 32;

type UserMap = Arc<DashMap<String, mpsc::Sender<ServerMessage>>>;

pub async fn run(listener: TcpListener, token: CancellationToken) {
    let users: UserMap = Arc::new(DashMap::new());
    info!("listening on {}", listener.local_addr().unwrap());

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        info!(%addr, "connection accepted");
                        let child = token.child_token();
                        tokio::spawn(handle_connection(stream, users.clone(), child));
                    }
                    Err(e) => error!("accept failed: {e}"),
                }
            }
            _ = token.cancelled() => {
                info!("shutting down");
                break;
            }
        }
    }
}

async fn handle_connection(stream: TcpStream, users: UserMap, token: CancellationToken) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    let username = match handshake(&mut lines, &mut writer, &users).await {
        Some(name) => name,
        None => return,
    };

    broadcast(
        &users,
        &username,
        ServerMessage::Joined {
            username: username.clone(),
        },
    )
    .await;

    let write_token = token.clone();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(64);

    // Replace the placeholder sender inserted during handshake with the real one.
    users.insert(username.clone(), tx);

    let write_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if writer.write_all(encode(&msg).as_bytes()).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = write_token.cancelled() => break,
            }
        }
    });

    loop {
        tokio::select! {
            result = lines.next_line() => {
                match result {
                    Ok(Some(line)) => match decode::<ClientMessage>(&line) {
                        Ok(ClientMessage::Send { content }) => {
                            broadcast(
                                &users,
                                &username,
                                ServerMessage::Message {
                                    username: username.clone(),
                                    content,
                                },
                            )
                            .await;
                        }
                        Ok(ClientMessage::Leave) => break,
                        Ok(ClientMessage::Join { .. }) => {
                            warn!(user = %username, "duplicate join ignored");
                        }
                        Err(e) => {
                            warn!(user = %username, "malformed message: {e}");
                        }
                    },
                    _ => break,
                }
            }
            _ = token.cancelled() => break,
        }
    }

    users.remove(&username);
    broadcast(
        &users,
        &username,
        ServerMessage::Left {
            username: username.clone(),
        },
    )
    .await;
    write_handle.abort();
}

async fn handshake(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    users: &UserMap,
) -> Option<String> {
    let line = match lines.next_line().await {
        Ok(Some(line)) => line,
        _ => return None,
    };

    let username = match decode::<ClientMessage>(&line) {
        Ok(ClientMessage::Join { username }) => username,
        _ => {
            let _ = send_error(writer, "first message must be a join").await;
            return None;
        }
    };

    let username = username.trim().to_string();

    if username.is_empty() {
        let _ = send_error(writer, "username cannot be empty").await;
        return None;
    }

    if username.len() > MAX_USERNAME_LEN {
        let _ = send_error(writer, "username too long").await;
        return None;
    }

    // Atomic check-and-insert to prevent TOCTOU race.
    match users.entry(username.clone()) {
        dashmap::mapref::entry::Entry::Occupied(_) => {
            let _ = send_error(writer, "username already taken").await;
            None
        }
        dashmap::mapref::entry::Entry::Vacant(entry) => {
            // Insert a placeholder sender; handle_connection will replace it
            // with the real channel after broadcasting the join notification.
            let (tx, _rx) = mpsc::channel(1);
            entry.insert(tx);
            Some(username)
        }
    }
}

async fn send_error(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    reason: &str,
) -> tokio::io::Result<()> {
    let msg = encode(&ServerMessage::Error {
        reason: reason.into(),
    });
    writer.write_all(msg.as_bytes()).await
}

async fn broadcast(users: &UserMap, sender: &str, msg: ServerMessage) {
    // Collect senders first to avoid holding DashMap read guards across await points.
    let recipients: Vec<_> = users
        .iter()
        .filter(|entry| entry.key() != sender)
        .map(|entry| entry.value().clone())
        .collect();

    for tx in recipients {
        let _ = tx.send(msg.clone()).await;
    }
}
