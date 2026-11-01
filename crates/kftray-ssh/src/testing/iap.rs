//! Fake IAP tunnel service: a WebSocket server on 127.0.0.1 that speaks the
//! relay protocol and bridges each session to one upstream TCP address.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};
use std::sync::{
    Arc,
    Mutex,
    MutexGuard,
    PoisonError,
};

use futures::{
    SinkExt,
    StreamExt,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::tcp::{
    OwnedReadHalf,
    OwnedWriteHalf,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::sync::{
    Notify,
    watch,
};
use tokio::task::{
    JoinHandle,
    JoinSet,
};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse,
    Request,
    Response,
};
use tokio_tungstenite::tungstenite::http::header::{
    AUTHORIZATION,
    ORIGIN,
    SEC_WEBSOCKET_PROTOCOL,
};
use tokio_tungstenite::tungstenite::http::{
    HeaderMap,
    HeaderValue,
    StatusCode,
};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use crate::transport::iap::consts::{
    ACK_THRESHOLD,
    CONNECT,
    MAX_DATA_FRAME,
    ORIGIN as ORIGIN_VALUE,
};
use crate::transport::iap::consts::{
    PATH_ROOT,
    RECONNECT,
    SUBPROTOCOL,
};
use crate::transport::iap::frame::{
    self,
    Frame,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a handshake asked for.
enum Opening {
    Connect,
    Reconnect { sid: String, ack: u64 },
}

/// The frame that opens a connection, built once the session is exclusively ours.
enum First {
    Sid(String),
    Ack,
}

/// Per-session state that outlives WebSocket connections.
struct Sid {
    read: tokio::sync::Mutex<OwnedReadHalf>,
    write: tokio::sync::Mutex<OwnedWriteHalf>,
    state: Mutex<SidState>,
}

#[derive(Default)]
struct SidState {
    /// Absolute offset of `down[0]`.
    down_base: u64,
    /// Upstream bytes the client has not acknowledged yet.
    down: Vec<u8>,
    /// Bytes received from the client and written upstream.
    up_total: u64,
    up_acked: u64,
}

impl SidState {
    fn acknowledge(&mut self, acked: u64) {
        let n = acked
            .saturating_sub(self.down_base)
            .min(self.down.len() as u64);
        self.down.drain(..n as usize);
        self.down_base += n;
    }
}

struct Shared {
    upstream: SocketAddr,
    token: String,
    connects: AtomicUsize,
    headers: Mutex<Vec<HeaderMap>>,
    uris: Mutex<Vec<String>>,
    acks: Mutex<Vec<u64>>,
    reject: Mutex<Option<u16>>,
    pending_close: Mutex<Option<u16>>,
    wake: Notify,
    drop_tx: watch::Sender<u64>,
    sids: Mutex<HashMap<String, Arc<Sid>>>,
}

/// IAP stand-in. Dropping it stops the server and every session.
pub struct FakeIap {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept: JoinHandle<()>,
}

impl FakeIap {
    /// `upstream` is where each new session connects (the sshd); `token` is
    /// the bearer token the handshake must present.
    pub async fn start(upstream: SocketAddr, token: impl Into<String>) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            upstream,
            token: token.into(),
            connects: AtomicUsize::new(0),
            headers: Mutex::default(),
            uris: Mutex::default(),
            acks: Mutex::default(),
            reject: Mutex::default(),
            pending_close: Mutex::default(),
            wake: Notify::new(),
            drop_tx: watch::channel(0).0,
            sids: Mutex::default(),
        });
        let accept = tokio::spawn(accept_loop(listener, shared.clone()));
        Ok(Self {
            addr,
            shared,
            accept,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Endpoint to give to `IapTransport::new`.
    pub fn endpoint(&self) -> String {
        format!("ws://{}", self.addr)
    }

    /// Drops every live WebSocket without a close frame.
    pub fn drop_connections(&self) {
        self.shared
            .drop_tx
            .send_modify(|generation| *generation += 1);
    }

    /// Closes one WebSocket with `code`, once: the live connection if there
    /// is one, otherwise the next one right after its handshake (before any frame).
    pub fn close_next_with(&self, code: u16) {
        *lock(&self.shared.pending_close) = Some(code);
        self.shared.wake.notify_one();
    }

    /// Answers every following handshake with this HTTP status.
    pub fn reject_handshake_with(&self, status: u16) {
        *lock(&self.shared.reject) = Some(status);
    }

    /// Request headers of every handshake received, rejected ones included.
    pub fn received_headers(&self) -> Vec<HeaderMap> {
        lock(&self.shared.headers).clone()
    }

    /// Path and query of every handshake received.
    pub fn received_uris(&self) -> Vec<String> {
        lock(&self.shared.uris).clone()
    }

    /// Values of the ACK frames received from clients, in order.
    pub fn acks_received(&self) -> Vec<u64> {
        lock(&self.shared.acks).clone()
    }

    /// Handshakes received, `/v4/connect` and `/v4/reconnect` alike.
    pub fn connect_count(&self) -> usize {
        self.shared.connects.load(Ordering::SeqCst)
    }
}

impl Drop for FakeIap {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    // Dropped with this task, which aborts every handler.
    let mut handlers = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    handlers.spawn(serve(stream, shared.clone()));
                }
                Err(_) => return,
            },
            Some(_) = handlers.join_next() => {}
        }
    }
}

fn error_response(status: u16) -> ErrorResponse {
    let mut response = ErrorResponse::new(None);
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    response
}

/// Validates the handshake like IAP would and records what the client sent.
#[allow(clippy::result_large_err)] // the error type is dictated by tungstenite's callback
fn check_handshake(
    shared: &Shared, request: &Request, mut response: Response,
) -> Result<(Response, Opening), ErrorResponse> {
    shared.connects.fetch_add(1, Ordering::SeqCst);
    lock(&shared.headers).push(request.headers().clone());
    let uri = request.uri();
    lock(&shared.uris).push(
        uri.path_and_query()
            .map_or_else(String::new, |p| p.to_string()),
    );
    if let Some(status) = *lock(&shared.reject) {
        return Err(error_response(status));
    }

    let headers = request.headers();
    let header = |name| headers.get(name).and_then(|value| value.to_str().ok());
    if header(AUTHORIZATION) != Some(format!("Bearer {}", shared.token).as_str()) {
        return Err(error_response(401));
    }
    let subprotocol_ok = header(SEC_WEBSOCKET_PROTOCOL)
        .is_some_and(|value| value.split(',').any(|p| p.trim() == SUBPROTOCOL));
    if header(ORIGIN) != Some(ORIGIN_VALUE) || !subprotocol_ok {
        return Err(error_response(400));
    }

    let query: HashMap<&str, &str> = uri
        .query()
        .unwrap_or("")
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect();
    let bad_request = || error_response(400);
    if query.get("newWebsocket") != Some(&"True") {
        return Err(bad_request());
    }
    let opening = if uri.path() == format!("{PATH_ROOT}/{CONNECT}") {
        if !query.contains_key("project") || !query.contains_key("port") {
            return Err(bad_request());
        }
        Opening::Connect
    } else if uri.path() == format!("{PATH_ROOT}/{RECONNECT}") {
        // The reconnect query carries exactly sid, ack, newWebsocket and the zone or region.
        let scoped = query.contains_key("zone") || query.contains_key("region");
        let (Some(sid), Some(ack)) = (query.get("sid"), query.get("ack")) else {
            return Err(bad_request());
        };
        let (true, 4, Ok(ack)) = (scoped, query.len(), ack.parse::<u64>()) else {
            return Err(bad_request());
        };
        if !lock(&shared.sids).contains_key(*sid) {
            return Err(error_response(404));
        }
        Opening::Reconnect {
            sid: (*sid).to_owned(),
            ack,
        }
    } else {
        return Err(error_response(404));
    };
    response.headers_mut().insert(
        SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static(SUBPROTOCOL),
    );
    Ok((response, opening))
}

fn close(code: u16) -> Message {
    Message::Close(Some(CloseFrame {
        code: CloseCode::from(code),
        reason: "".into(),
    }))
}

fn binary(frame: Vec<u8>) -> Message {
    Message::Binary(frame.into())
}

#[allow(clippy::result_large_err)] // callback signature is dictated by tungstenite
async fn serve(stream: TcpStream, shared: Arc<Shared>) {
    let mut dropped = shared.drop_tx.subscribe();
    let slot = Arc::new(Mutex::new(None));
    let callback = {
        let (shared, slot) = (shared.clone(), slot.clone());
        move |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            let (response, opening) = check_handshake(&shared, request, response)?;
            *lock(&slot) = Some(opening);
            Ok(response)
        }
    };
    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
        return;
    };
    let Some(opening) = lock(&slot).take() else {
        return;
    };
    let pending = lock(&shared.pending_close).take();
    if let Some(code) = pending {
        let _ = ws.send(close(code)).await;
        return;
    }

    let (sid_id, sid, first, mut sent) = match opening {
        Opening::Connect => {
            let Ok(upstream) = TcpStream::connect(shared.upstream).await else {
                let _ = ws.send(close(4003)).await;
                return;
            };
            let (read, write) = upstream.into_split();
            let id = format!("{:016x}", rand::random::<u64>());
            let sid = Arc::new(Sid {
                read: read.into(),
                write: write.into(),
                state: Mutex::default(),
            });
            lock(&shared.sids).insert(id.clone(), sid.clone());
            (id.clone(), sid, First::Sid(id), 0)
        }
        Opening::Reconnect { sid: id, ack } => {
            let found = lock(&shared.sids).get(&id).cloned();
            let Some(sid) = found else {
                let _ = ws.send(close(4047)).await;
                return;
            };
            let sent = {
                let mut state = lock(&sid.state);
                state.acknowledge(ack);
                let end = state.down_base + state.down.len() as u64;
                ack.clamp(state.down_base, end)
            };
            (id, sid, First::Ack, sent)
        }
    };

    // A previous connection of this session may still hold the halves, and may still be
    // handling frames it received before it was dropped. Only after it let go is the
    // upstream byte count final, so it must be read after taking the locks.
    let mut upstream_read = sid.read.lock().await;
    let mut upstream_write = sid.write.lock().await;
    let mut upstream_eof = false;
    let first = match first {
        First::Sid(id) => frame::encode_connect_success_sid(id.as_bytes()),
        First::Ack => frame::encode_reconnect_success_ack(lock(&sid.state).up_total),
    };
    if ws.send(binary(first)).await.is_err() {
        return;
    }
    let mut buf = vec![0u8; MAX_DATA_FRAME];
    loop {
        // Bytes read from upstream but not yet sent on this connection (also the replay).
        let chunk = {
            let state = lock(&sid.state);
            let end = state.down_base + state.down.len() as u64;
            (sent < end).then(|| {
                let from = (sent - state.down_base) as usize;
                let to = (from + MAX_DATA_FRAME).min(state.down.len());
                state.down[from..to].to_vec()
            })
        };
        if let Some(chunk) = chunk {
            if ws.send(binary(frame::encode_data(&chunk))).await.is_err() {
                return;
            }
            sent += chunk.len() as u64;
            continue;
        }

        tokio::select! {
            biased;
            _ = dropped.changed() => return,
            _ = shared.wake.notified() => {
                let code = lock(&shared.pending_close).take();
                if let Some(code) = code {
                    let _ = ws.send(close(code)).await;
                    return;
                }
            }
            message = ws.next() => match message {
                Some(Ok(Message::Binary(payload))) => match frame::decode(&payload) {
                    Ok((Frame::Data(data), _)) => {
                        if upstream_write.write_all(data).await.is_err() {
                            return;
                        }
                        let ack = {
                            let mut state = lock(&sid.state);
                            state.up_total += data.len() as u64;
                            (state.up_total - state.up_acked > ACK_THRESHOLD).then(|| {
                                state.up_acked = state.up_total;
                                state.up_total
                            })
                        };
                        if let Some(total) = ack
                            && ws.send(binary(frame::encode_ack(total))).await.is_err()
                        {
                            return;
                        }
                    }
                    Ok((Frame::Ack(acked), _)) => {
                        lock(&shared.acks).push(acked);
                        lock(&sid.state).acknowledge(acked);
                    }
                    _ => {}
                },
                Some(Ok(Message::Close(_))) => {
                    let _ = upstream_write.shutdown().await;
                    lock(&shared.sids).remove(&sid_id);
                    return;
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return,
            },
            read = upstream_read.read(&mut buf), if !upstream_eof => match read {
                Ok(n) if n > 0 => lock(&sid.state).down.extend_from_slice(&buf[..n]),
                // The destination only half-closed: the client may still be uploading, so the
                // tunnel stays up until the client closes it.
                _ => upstream_eof = true,
            },
        }
    }
}
