//! Low-level WebSocket plumbing shared by the client and the service probe.

use std::time::Duration;

use futures_util::StreamExt;
use poker_protocol::{Hello, ServerMessage, ServerParseError, parse_server_message};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::config::validate_ws_url;
use crate::error::ClientError;

/// WebSocket stream type used by the client.
pub(crate) type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Opens the TCP/WebSocket connection; the URL scheme is validated first.
pub(crate) async fn open_ws(url: &str) -> Result<WsStream, ClientError> {
    validate_ws_url(url)?;
    match connect_async(url).await {
        Ok((stream, _response)) => Ok(stream),
        Err(error) => Err(ClientError::WebSocket(error.to_string())),
    }
}

/// Reads frames until the server hello arrives (the protocol requires hello
/// as the first server frame, before any request is processed).
pub(crate) async fn read_hello(stream: &mut WsStream) -> Result<Hello, ClientError> {
    loop {
        match stream.next().await {
            Some(Ok(Message::Text(text))) => {
                let raw = text.as_str().to_owned();
                return match parse_server_message(&raw) {
                    Ok(ServerMessage::Hello(hello)) => Ok(hello),
                    Ok(other) => Err(ClientError::Protocol {
                        raw,
                        error: ServerParseError::InvalidShape {
                            message_type: other.message_type().to_owned(),
                            detail: "expected hello as the first server frame".to_owned(),
                        },
                    }),
                    Err(error) => Err(ClientError::Protocol { raw, error }),
                };
            }
            Some(Ok(
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) | Message::Binary(_),
            )) => {}
            Some(Ok(Message::Close(_))) | None => {
                return Err(ClientError::WebSocket(
                    "connection closed before hello".to_owned(),
                ));
            }
            Some(Err(error)) => return Err(ClientError::WebSocket(error.to_string())),
        }
    }
}

/// Closes a stream politely, ignoring errors and bounded by a short timeout.
pub(crate) async fn close_stream(mut stream: WsStream) {
    let _ = tokio::time::timeout(Duration::from_millis(200), stream.close(None)).await;
}

/// Converts a handshake timeout duration into the error payload.
pub(crate) fn handshake_timeout_error(timeout: Duration) -> ClientError {
    ClientError::HandshakeTimeout {
        timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
    }
}
