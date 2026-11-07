//! Live topic updates over Server-Sent Events (`GET /topics/{id}/events`).
//!
//! Routes publish an event when a comment is added or changes, or a topic's
//! status changes; every open stream for that topic gets it. Events only say
//! *what* changed; clients fetch the data with the normal API (which applies
//! the usual visibility rules).
//!
//! In-process only: with several backend processes, a client only hears about
//! changes made through the process it is connected to (clients still poll as
//! a fallback, see the client's useTopic).

use futures_util::stream::{self, Stream, StreamExt};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast::{self, error::RecvError};

/// Sent on idle streams so proxies (nginx `proxy_read_timeout` 60s by default,
/// Cloudflare 100s) keep them open.
const KEEPALIVE: Duration = Duration::from_secs(25);
/// Events buffered per stream before a slow client gets `resync`.
const CHANNEL_SIZE: usize = 256;
/// Open streams per client address (one per open topic tab), so one client
/// cannot take every slot.
const MAX_STREAMS_PER_IP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    /// A new comment (`id` = comment id).
    Comment,
    /// A comment changed, e.g. hidden (`id` = comment id).
    CommentUpdated,
    /// The topic's status changed (closed, pinned, ...).
    Topic,
}

impl EventKind {
    fn name(self) -> &'static str {
        match self {
            EventKind::Comment => "comment",
            EventKind::CommentUpdated => "comment_updated",
            EventKind::Topic => "topic",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub topic_id: i32,
    pub kind: EventKind,
    pub id: i32,
}

pub struct Events {
    sender: broadcast::Sender<Event>,
    streams: Arc<AtomicUsize>,
    per_ip: Arc<Mutex<HashMap<IpAddr, usize>>>,
    max_streams: usize,
}

/// Decrements the open stream counts when a stream ends.
struct StreamGuard {
    streams: Arc<AtomicUsize>,
    per_ip: Arc<Mutex<HashMap<IpAddr, usize>>>,
    ip: IpAddr,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.streams.fetch_sub(1, Ordering::Relaxed);
        let mut per_ip = self.per_ip.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = per_ip.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                per_ip.remove(&self.ip);
            }
        }
    }
}

impl Events {
    pub fn new(max_streams: usize) -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_SIZE);
        Events {
            sender,
            streams: Arc::new(AtomicUsize::new(0)),
            per_ip: Arc::new(Mutex::new(HashMap::new())),
            max_streams,
        }
    }

    pub fn publish(&self, topic_id: i32, kind: EventKind, id: i32) {
        // An error only means nobody is listening.
        let _ = self.sender.send(Event { topic_id, kind, id });
    }

    pub fn open_streams(&self) -> usize {
        self.streams.load(Ordering::Relaxed)
    }

    /// The SSE body for one topic, or `None` when too many streams are open,
    /// in all or from `ip` (the client then falls back to polling).
    pub fn stream(
        &self,
        topic_id: i32,
        ip: IpAddr,
    ) -> Option<impl Stream<Item = Result<actix_web::web::Bytes, actix_web::Error>> + use<>> {
        {
            let mut per_ip = self.per_ip.lock().unwrap_or_else(|e| e.into_inner());
            let n = per_ip.entry(ip).or_insert(0);
            if *n >= MAX_STREAMS_PER_IP {
                return None;
            }
            if self.streams.fetch_add(1, Ordering::Relaxed) >= self.max_streams {
                self.streams.fetch_sub(1, Ordering::Relaxed);
                if *n == 0 {
                    per_ip.remove(&ip);
                }
                return None;
            }
            *n += 1;
        }
        let guard = StreamGuard {
            streams: self.streams.clone(),
            per_ip: self.per_ip.clone(),
            ip,
        };
        let receiver = self.sender.subscribe();
        // Tell EventSource to retry after 5 s, and flush headers right away.
        let hello = actix_web::web::Bytes::from_static(b"retry: 5000\n: connected\n\n");
        let events = stream::unfold((receiver, guard), move |(mut receiver, guard)| async move {
            loop {
                let message = tokio::select! {
                    received = receiver.recv() => match received {
                        Ok(event) if event.topic_id == topic_id => {
                            format!("event: {}\ndata: {{\"id\":{}}}\n\n", event.kind.name(), event.id)
                        }
                        Ok(_) => continue,
                        // Missed events: the client reloads what it shows.
                        Err(RecvError::Lagged(_)) => "event: resync\ndata: {}\n\n".to_owned(),
                        Err(RecvError::Closed) => return None,
                    },
                    _ = tokio::time::sleep(KEEPALIVE) => ": ping\n\n".to_owned(),
                };
                return Some((Ok(actix_web::web::Bytes::from(message)), (receiver, guard)));
            }
        });
        Some(stream::once(async move { Ok(hello) }).chain(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    async fn next_text<S>(stream: &mut S) -> String
    where
        S: Stream<Item = Result<actix_web::web::Bytes, actix_web::Error>> + Unpin,
    {
        let bytes = stream.next().await.unwrap().unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[actix_web::test]
    async fn test_stream() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let events = Events::new(2);
        let mut a = Box::pin(events.stream(1, ip).unwrap());
        let b = events.stream(2, ip).unwrap();
        assert!(events.stream(3, ip).is_none(), "limit reached");
        assert_eq!(events.open_streams(), 2);
        drop(b);
        assert_eq!(events.open_streams(), 1);

        assert!(next_text(&mut a).await.starts_with("retry: 5000"));
        events.publish(2, EventKind::Comment, 7); // other topic: skipped
        events.publish(1, EventKind::Comment, 8);
        assert_eq!(
            next_text(&mut a).await,
            "event: comment\ndata: {\"id\":8}\n\n"
        );
        events.publish(1, EventKind::CommentUpdated, 8);
        assert_eq!(
            next_text(&mut a).await,
            "event: comment_updated\ndata: {\"id\":8}\n\n"
        );
        events.publish(1, EventKind::Topic, 1);
        assert_eq!(
            next_text(&mut a).await,
            "event: topic\ndata: {\"id\":1}\n\n"
        );
        drop(a);
        assert_eq!(events.open_streams(), 0);
    }

    #[actix_web::test]
    async fn test_per_ip_limit() {
        let events = Events::new(100);
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        let mut held: Vec<_> = (0..MAX_STREAMS_PER_IP)
            .map(|_| events.stream(1, a).unwrap())
            .collect();
        assert!(events.stream(1, a).is_none(), "per-address limit");
        assert!(events.stream(1, b).is_some(), "others are not affected");
        held.pop();
        assert!(
            events.stream(1, a).is_some(),
            "a slot frees up when a stream ends"
        );
        drop(held);
        assert!(events.per_ip.lock().unwrap().len() <= 1);
    }

    #[actix_web::test]
    async fn test_lagged_stream_resyncs() {
        let events = Events::new(10);
        let mut a = Box::pin(events.stream(1, "10.0.0.1".parse().unwrap()).unwrap());
        next_text(&mut a).await;
        for i in 0..(CHANNEL_SIZE as i32 + 10) {
            events.publish(1, EventKind::Comment, i);
        }
        assert_eq!(next_text(&mut a).await, "event: resync\ndata: {}\n\n");
    }
}
