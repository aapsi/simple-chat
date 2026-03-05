use clap::Parser;
use protocol::{decode, encode, ClientMessage, ServerMessage};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

#[derive(Parser)]
#[command(about = "Simple chat client")]
struct Args {
    #[arg(long, default_value = "127.0.0.1", env = "CHAT_HOST")]
    host: String,

    #[arg(long, default_value = "8080", env = "CHAT_PORT")]
    port: u16,

    #[arg(long, env = "CHAT_USERNAME")]
    username: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let stream = TcpStream::connect(format!("{}:{}", args.host, args.port)).await?;
    let (reader, mut writer) = stream.into_split();

    let join_msg = encode(&ClientMessage::Join {
        username: args.username.clone(),
    });
    writer.write_all(join_msg.as_bytes()).await?;

    let mut server_lines = BufReader::new(reader).lines();
    let (disconnect_tx, disconnect_rx) = oneshot::channel::<()>();

    let read_handle = tokio::spawn(async move {
        while let Ok(Some(line)) = server_lines.next_line().await {
            match decode::<ServerMessage>(&line) {
                Ok(ServerMessage::Message { username, content }) => {
                    println!("[{username}] {content}");
                }
                Ok(ServerMessage::Joined { username }) => {
                    println!("* {username} joined the chat");
                }
                Ok(ServerMessage::Left { username }) => {
                    println!("* {username} left the chat");
                }
                Ok(ServerMessage::Error { reason }) => {
                    eprintln!("error: {reason}");
                    break;
                }
                Err(e) => {
                    eprintln!("malformed server message: {e}");
                }
            }
        }
        let _ = disconnect_tx.send(());
    });

    let mut stdin = BufReader::new(io::stdin()).lines();

    tokio::pin!(disconnect_rx);
    loop {
        tokio::select! {
            line = stdin.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let trimmed = line.trim().to_string();

                        if trimmed.eq_ignore_ascii_case("leave") {
                            let _ = writer
                                .write_all(encode(&ClientMessage::Leave).as_bytes())
                                .await;
                            break;
                        }

                        if let Some(content) = trimmed.strip_prefix("send ") {
                            if content.is_empty() {
                                println!("usage: send <message>");
                                continue;
                            }
                            let msg = ClientMessage::Send {
                                content: content.to_string(),
                            };
                            writer.write_all(encode(&msg).as_bytes()).await?;
                        } else if !trimmed.is_empty() {
                            println!("usage: send <message> | leave");
                        }
                    }
                    _ => break,
                }
            }
            _ = &mut disconnect_rx => {
                eprintln!("server disconnected");
                break;
            }
        }
    }

    read_handle.abort();
    Ok(())
}
