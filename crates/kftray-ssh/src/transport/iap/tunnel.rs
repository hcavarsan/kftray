//! One IAP tunnel: an actor task owns the WebSocket, turns the local
//! duplex stream into DATA frames, acknowledges what it receives and, when
//! the socket drops, resumes the same session through `/v4/reconnect`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use bytes::{
    Buf,
    Bytes,
};
use futures::stream::SplitStream;
use futures::{
    SinkExt,
    StreamExt,
};
use log::{
    debug,
    info,
    warn,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
    DuplexStream,
    ReadHalf,
    WriteHalf,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{
    Instant,
    MissedTickBehavior,
    interval_at,
    timeout,
};
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use super::connect::{
    Dialer,
    Ws,
};
use super::consts::{
    ACK_IDLE_FLUSH,
    ACK_THRESHOLD,
    CLOSE_GRACE,
    DUPLEX_BUFFER,
    MAX_DATA_FRAME,
    OPEN_TIMEOUT,
    RECONNECT_BUDGET,
    RECONNECT_INITIAL,
    RECONNECT_JITTER,
    RECONNECT_MAX_SLEEP,
    RECONNECT_MULTIPLIER,
    RESEND_LIMIT,
};
use super::error::IapError;
use super::frame::{
    self,
    Frame,
};
use crate::error::SshError;

/// Opens the tunnel and returns the client half of the local stream.
pub(crate) async fn open(dialer: Arc<Dialer>) -> Result<DuplexStream, SshError> {
    let (ws, sid) = timeout(OPEN_TIMEOUT, async {
        let mut ws = dialer.connect().await?;
        let first = first_binary(&mut ws).await?;
        let sid = expect_sid(&first)?;
        Ok::<_, SshError>((ws, sid))
    })
    .await
    .map_err(|_| IapError::Timeout)??;
    info!("[iap {}] tunnel open", dialer.name());

    let (client, server) = tokio::io::duplex(DUPLEX_BUFFER);
    let (local_read, local_write) = tokio::io::split(server);
    tokio::spawn(
        Actor {
            dialer,
            sid,
            local_read,
            local_write,
            conn: Conn::new(ws),
            total_received: 0,
            last_ack_sent: 0,
            total_sent: 0,
            queue: VecDeque::new(),
            queued: 0,
            pending: VecDeque::new(),
            draining: false,
        }
        .run(),
    );
    Ok(client)
}

/// First binary message of a fresh WebSocket; a close frame becomes the error it means.
async fn first_binary(ws: &mut Ws) -> Result<Bytes, IapError> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Binary(payload))) => return Ok(payload),
            Some(Ok(Message::Close(Some(frame)))) => {
                return Err(IapError::from_close(u16::from(frame.code), &frame.reason));
            }
            Some(Ok(Message::Close(None))) => {
                return Err(IapError::Io("closed before the first frame".into()));
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(IapError::from_ws(e)),
            None => {
                return Err(IapError::Io(
                    "connection ended before the first frame".into(),
                ));
            }
        }
    }
}

fn expect_sid(first: &[u8]) -> Result<String, IapError> {
    match frame::decode(first) {
        Ok((Frame::ConnectSuccessSid(sid), _)) if !sid.is_empty() => {
            String::from_utf8(sid.to_vec())
                .map_err(|_| IapError::Protocol("the session id is not text".into()))
        }
        Ok((other, _)) => Err(IapError::Protocol(format!(
            "expected CONNECT_SUCCESS_SID, got {}",
            other.name()
        ))),
        Err(e) => Err(IapError::Protocol(e.to_string())),
    }
}

fn expect_reconnect_ack(first: &[u8]) -> Result<u64, IapError> {
    match frame::decode(first) {
        Ok((Frame::ReconnectSuccessAck(ack), _)) => Ok(ack),
        Ok((other, _)) => Err(IapError::Protocol(format!(
            "expected RECONNECT_SUCCESS_ACK, got {}",
            other.name()
        ))),
        Err(e) => Err(IapError::Protocol(e.to_string())),
    }
}

/// `min(1500 ms × 1.1^attempt + jitter, 20 s)`
fn backoff(attempt: u32, jitter_ms: u64) -> Duration {
    let grown =
        RECONNECT_INITIAL.as_millis() as f64 * RECONNECT_MULTIPLIER.powi(attempt.min(200) as i32);
    let capped = (grown + jitter_ms as f64).min(RECONNECT_MAX_SLEEP.as_millis() as f64);
    Duration::from_millis(capped as u64)
}

fn close_message() -> Message {
    Message::Close(Some(CloseFrame {
        code: CloseCode::Normal,
        reason: "".into(),
    }))
}

/// A live WebSocket. Writes go through a dedicated task so that the actor
/// keeps reading while the socket is backpressured; otherwise both ends
/// could block in send at once and deadlock.
struct Conn {
    out: mpsc::UnboundedSender<Message>,
    stream: SplitStream<Ws>,
    writer: JoinHandle<()>,
}

impl Conn {
    fn new(ws: Ws) -> Self {
        let (mut sink, stream) = ws.split();
        let (out, mut queue) = mpsc::unbounded_channel::<Message>();
        // Unbounded is safe: data in it is capped by RESEND_LIMIT and ACKs
        // are one per received frame.
        let writer = tokio::spawn(async move {
            while let Some(message) = queue.recv().await {
                let closing = matches!(message, Message::Close(_));
                if sink.send(message).await.is_err() || closing {
                    break;
                }
            }
        });
        Self {
            out,
            stream,
            writer,
        }
    }

    /// Sends Close(1000) and gives it a moment to leave.
    async fn close(&mut self) {
        let _ = self.out.send(close_message());
        if !self.writer.is_finished() {
            let _ = timeout(CLOSE_GRACE, &mut self.writer).await;
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.writer.abort();
    }
}

enum Step {
    Continue,
    Resume(String),
    Stop,
}

struct Actor {
    dialer: Arc<Dialer>,
    sid: String,
    /// Server half of the stream handed to the caller, split so reads and writes are
    /// separate `select!` arms.
    local_read: ReadHalf<DuplexStream>,
    local_write: WriteHalf<DuplexStream>,
    conn: Conn,
    /// Bytes delivered to the local stream so far; sent as `ack` on reconnect.
    total_received: u64,
    last_ack_sent: u64,
    /// Bytes read from the local stream so far.
    total_sent: u64,
    /// Sent but unacknowledged chunks: (offset of first byte, payload).
    queue: VecDeque<(u64, Bytes)>,
    queued: usize,
    /// Received DATA payloads not yet written to the local stream, oldest first. Bounded in
    /// practice by the flow control of the protocol carried over the tunnel.
    pending: VecDeque<Bytes>,
    /// IAP closed the tunnel; only `pending` is left to deliver.
    draining: bool,
}

impl Actor {
    async fn run(mut self) {
        let mut idle = interval_at(Instant::now() + ACK_IDLE_FLUSH, ACK_IDLE_FLUSH);
        idle.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut buf = vec![0u8; MAX_DATA_FRAME];
        loop {
            let step = tokio::select! {
                // Not biased, so a busy socket cannot starve the local side. Delivering a DATA
                // frame to the local stream is an arm of its own: awaiting it inline would stop
                // reading the local side, and if that side is blocked writing to us too (russh
                // flushes without reading) both would hang. The socket is read meanwhile too: the
                // ACKs that free the resend queue sit behind the data we cannot deliver yet.
                written = self.local_write.write(self.pending.front().map_or(&[][..], |b| &b[..])),
                    if !self.pending.is_empty() => {
                    self.on_written(written)
                }
                message = self.conn.stream.next(), if !self.draining => {
                    self.on_message(message)
                }
                read = self.local_read.read(&mut buf), if self.queued < RESEND_LIMIT => {
                    self.on_local(read, &buf)
                }
                _ = idle.tick() => {
                    if self.total_received > self.last_ack_sent {
                        self.send_ack();
                    }
                    Step::Continue
                }
                _ = &mut self.conn.writer, if !self.draining => {
                    Step::Resume("the write half failed".into())
                }
            };
            match step {
                Step::Continue => {}
                Step::Stop => break,
                Step::Resume(reason) => {
                    if !self.resume(&reason).await {
                        break;
                    }
                }
            }
        }
        // EOF for the reader of the stream, then a clean close of the socket.
        let _ = self.local_write.shutdown().await;
        self.conn.close().await;
        debug!("[iap {}] tunnel closed", self.dialer.name());
    }

    /// Accounts for `written` bytes delivered to the local stream.
    fn on_written(&mut self, written: std::io::Result<usize>) -> Step {
        match written {
            Ok(n) if n > 0 => {
                if let Some(front) = self.pending.front_mut() {
                    front.advance(n);
                    if front.is_empty() {
                        self.pending.pop_front();
                    }
                }
                if self.draining && self.pending.is_empty() {
                    return Step::Stop;
                }
                Step::Continue
            }
            _ => Step::Stop,
        }
    }

    fn on_message(&mut self, message: Option<Result<Message, WsError>>) -> Step {
        match message {
            Some(Ok(Message::Binary(payload))) => self.on_binary(&payload),
            Some(Ok(Message::Close(frame))) => match self.on_close(frame) {
                // Data received before the close still goes to the caller.
                Step::Stop if !self.pending.is_empty() => {
                    self.draining = true;
                    Step::Continue
                }
                step => step,
            },
            Some(Ok(_)) => Step::Continue,
            Some(Err(e)) => Step::Resume(e.to_string()),
            None => Step::Resume("the connection ended without a close frame".into()),
        }
    }

    fn on_binary(&mut self, payload: &Bytes) -> Step {
        let dialer = self.dialer.clone();
        let name = dialer.name();
        let frame = match frame::decode(payload) {
            Ok((frame, used)) => {
                if used < payload.len() {
                    debug!(
                        "[iap {name}] ignoring {} trailing bytes",
                        payload.len() - used
                    );
                }
                frame
            }
            Err(e) => {
                warn!("[iap {name}] bad frame: {e}");
                return Step::Stop;
            }
        };
        match frame {
            Frame::Data(data) => {
                debug!("[iap {name}] DATA {} bytes", data.len());
                self.total_received += data.len() as u64;
                if self.total_received - self.last_ack_sent > ACK_THRESHOLD {
                    self.send_ack();
                }
                // Delivered by the run loop's write arm, which keeps draining the local side
                // meanwhile; the socket keeps being read so ACKs are never stuck behind data.
                self.pending.push_back(payload.slice_ref(data));
            }
            Frame::Ack(acked) => {
                debug!("[iap {name}] ACK {acked}");
                self.trim_queue(acked);
            }
            Frame::ConnectSuccessSid(_) | Frame::ReconnectSuccessAck(_) => {
                warn!("[iap {name}] unexpected {} frame", frame.name());
                return Step::Stop;
            }
            Frame::Unknown(tag) => debug!("[iap {name}] ignoring frame with tag {tag}"),
        }
        Step::Continue
    }

    fn on_close(&self, frame: Option<CloseFrame>) -> Step {
        let name = self.dialer.name();
        let code = frame.as_ref().map_or(1005, |f| u16::from(f.code));
        match code {
            1000 | 4010 => {
                debug!("[iap {name}] closed by IAP ({code})");
                Step::Stop
            }
            4004 => Step::Resume("IAP asked to reconnect (4004)".into()),
            code if code < 4000 => Step::Resume(format!("closed with code {code}")),
            code => {
                // The next connect() from the caller surfaces the typed error.
                let reason = frame.as_ref().map_or("", |f| f.reason.as_str());
                warn!("[iap {name}] {}", IapError::from_close(code, reason));
                Step::Stop
            }
        }
    }

    fn on_local(&mut self, read: std::io::Result<usize>, buf: &[u8]) -> Step {
        let n = match read {
            Ok(n) if n > 0 => n,
            _ => {
                debug!("[iap {}] local side closed", self.dialer.name());
                return Step::Stop;
            }
        };
        let payload = Bytes::copy_from_slice(&buf[..n]);
        let frame = frame::encode_data(&payload);
        self.queue.push_back((self.total_sent, payload));
        self.total_sent += n as u64;
        self.queued += n;
        debug!("[iap {}] sending DATA {n} bytes", self.dialer.name());
        if self.conn.out.send(Message::Binary(frame.into())).is_err() {
            return Step::Resume("the write half failed".into());
        }
        Step::Continue
    }

    fn send_ack(&mut self) {
        let frame = frame::encode_ack(self.total_received);
        if self.conn.out.send(Message::Binary(frame.into())).is_ok() {
            self.last_ack_sent = self.total_received;
        }
    }

    /// Forgets queued bytes below `acked`, slicing a partly acknowledged chunk.
    fn trim_queue(&mut self, acked: u64) {
        while let Some((offset, chunk)) = self.queue.front_mut() {
            let len = chunk.len();
            if *offset + len as u64 <= acked {
                self.queued -= len;
                self.queue.pop_front();
                continue;
            }
            if acked > *offset {
                let cut = (acked - *offset) as usize;
                *chunk = chunk.slice(cut..);
                *offset = acked;
                self.queued -= cut;
            }
            break;
        }
    }

    /// Reconnects with backoff until it works, fails for good, the budget
    /// ends or the local side is gone. The local stream is not read meanwhile.
    async fn resume(&mut self, reason: &str) -> bool {
        let name = self.dialer.name().to_owned();
        warn!("[iap {name}] tunnel interrupted ({reason}); resuming");
        let started = Instant::now();
        let mut attempt = 0;
        while started.elapsed() < RECONNECT_BUDGET {
            let jitter = rand::random_range(0..RECONNECT_JITTER.as_millis() as u64);
            tokio::time::sleep(backoff(attempt, jitter)).await;
            // Writing nothing fails once the client half is dropped.
            if self.local_write.write(&[]).await.is_err() {
                return false;
            }
            match self.try_reconnect().await {
                Ok(()) => {
                    info!("[iap {name}] resumed after {:?}", started.elapsed());
                    return true;
                }
                Err(e) if e.is_retryable() => {
                    debug!("[iap {name}] reconnect attempt {attempt} failed: {e}");
                }
                Err(e) => {
                    warn!("[iap {name}] giving up on the tunnel: {e}");
                    return false;
                }
            }
            attempt += 1;
        }
        warn!("[iap {name}] could not resume within {RECONNECT_BUDGET:?}");
        false
    }

    async fn try_reconnect(&mut self) -> Result<(), SshError> {
        let (ws, server_ack) = timeout(OPEN_TIMEOUT, async {
            let mut ws = self
                .dialer
                .reconnect(&self.sid, self.total_received)
                .await?;
            let first = first_binary(&mut ws).await?;
            Ok::<_, SshError>((ws, expect_reconnect_ack(&first)?))
        })
        .await
        .map_err(|_| IapError::Timeout)??;
        if server_ack > self.total_sent {
            return Err(IapError::Protocol("IAP acknowledged bytes we never sent".into()).into());
        }
        self.trim_queue(server_ack);
        let conn = Conn::new(ws);
        for (_, chunk) in &self.queue {
            // A closed channel means the new writer already died; the next loop turn resumes again.
            let _ = conn
                .out
                .send(Message::Binary(frame::encode_data(chunk).into()));
        }
        self.conn = conn;
        // IAP learned our count from the reconnect URL.
        self.last_ack_sent = self.total_received;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_should_grow_by_the_multiplier_when_attempts_increase() {
        assert_eq!(backoff(0, 0), Duration::from_millis(1500));
        assert_eq!(backoff(1, 0), Duration::from_millis(1650));
        assert_eq!(backoff(0, 999), Duration::from_millis(2499));
    }

    #[test]
    fn backoff_should_cap_at_twenty_seconds_when_attempts_are_many() {
        assert_eq!(backoff(60, 0), Duration::from_secs(20));
        assert_eq!(backoff(u32::MAX, 999), Duration::from_secs(20));
    }

    #[test]
    fn expect_sid_should_reject_other_frames_when_first_frame_is_not_sid() {
        assert_eq!(
            expect_sid(&frame::encode_connect_success_sid(b"abc")).unwrap(),
            "abc"
        );
        assert!(expect_sid(&frame::encode_ack(1)).is_err());
        assert!(expect_sid(&frame::encode_connect_success_sid(b"")).is_err());
        assert!(expect_sid(&[0]).is_err());
    }
}
