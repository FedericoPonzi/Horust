//! A simple bus implementation: distributes the messages among the queues
//! There is one single input pipe (`public_sender` ; `receiver`). The sender side is shared among
//! all the publishers. The bus reads from the receiver, and publishes to all the `senders`.
//! This is a very simple wrapper around crossbeam, that allows multiple sender send messages which
//! will arrive to every receiver. For this reason, the message should implement Clone.
//!

use std::fmt::Formatter;
use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Receiver, Sender, unbounded};

struct Subscriber<T: Clone> {
    sender: Sender<Message<T>>,
    #[cfg(feature = "formal-trace")]
    id: u64,
}

/// Bus state shared between `Bus` and all `BusConnector` instances.
/// It contains all necessary components to send data and join the bus.
#[derive(Clone)]
pub struct SharedState<T>
where
    T: Clone,
{
    /// Bus input - sender side
    sender: Sender<Message<T>>,
    /// Bus output - all the senders
    senders: Arc<Mutex<Vec<Subscriber<T>>>>,
}

impl<T> SharedState<T>
where
    T: Clone,
{
    /// Add another connection to the bus
    pub fn join_bus(&self) -> BusConnector<T> {
        let mut senders = self.senders.lock().unwrap();

        let (sender, receiver) = unbounded();
        #[cfg(feature = "formal-trace")]
        let receiver_id = {
            let id = super::formal_trace::id();
            super::formal_trace::record("join", serde_json::json!({"receiver": id}));
            id
        };
        senders.push(Subscriber {
            sender,
            #[cfg(feature = "formal-trace")]
            id: receiver_id,
        });

        #[cfg(feature = "formal-trace")]
        let connector = BusConnector::new(receiver, self.clone(), receiver_id);
        #[cfg(not(feature = "formal-trace"))]
        let connector = BusConnector::new(receiver, self.clone());
        connector
    }
}

/// A simple bus implementation: distributes the messages among the queues
/// There is one single input pipe (`public_sender` ; `receiver`). The sender side is shared among
/// all the publishers. The bus reads from the receiver, and publishes to all the `senders`.
pub struct Bus<T>
where
    T: Clone,
{
    /// Bus state shared with all `BusConnector`. All necessary components to send data and join the bus.
    state: SharedState<T>,
    /// Bus input - receiver side
    receiver: Receiver<Message<T>>,
}

impl<T> Debug for Bus<T>
where
    T: Debug + Clone,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bus {{ senders.len(): {} }}",
            self.state.senders.lock().unwrap().len(),
        )
    }
}

impl<T> Default for Bus<T>
where
    T: Clone,
{
    fn default() -> Self {
        let (public_sender, receiver) = unbounded();
        Bus {
            state: SharedState {
                sender: public_sender,
                senders: Default::default(),
            },
            receiver,
        }
    }
}

impl<T> Bus<T>
where
    T: Clone,
{
    pub fn new() -> Self {
        // use the default implementation since no arguments are provided
        Self::default()
    }

    /// Blocking
    pub fn run(self) {
        self.dispatch();
    }

    /// Add another connection to the bus
    pub fn join_bus(&self) -> BusConnector<T> {
        self.state.join_bus()
    }

    /// Dispatching loop
    /// As soon as we don't have any senders it will exit
    fn dispatch(self) {
        drop(self.state.sender);
        for ev in self.receiver {
            Self::distribute(&self.state.senders, ev);
        }
    }

    fn distribute(subscribers: &Mutex<Vec<Subscriber<T>>>, ev: Message<T>) {
        let mut senders = subscribers.lock().unwrap();
        #[cfg(feature = "formal-trace")]
        super::formal_trace::record(
            "dispatch",
            serde_json::json!({
                "message": ev.id, "receivers": senders.iter().map(|s| s.id).collect::<Vec<_>>(),
            }),
        );
        senders.retain(|subscriber| subscriber.sender.send(ev.clone()).is_ok());
    }

    #[cfg(all(test, feature = "formal-trace"))]
    pub(crate) fn pump(&self) {
        for ev in self.receiver.try_iter() {
            Self::distribute(&self.state.senders, ev);
        }
    }
}

/// The payload with wrapped with some metadata
#[derive(Clone)]
struct Message<T>
where
    T: Clone,
{
    payload: T,
    #[cfg(feature = "formal-trace")]
    id: u64,
}

impl<T> Message<T>
where
    T: Clone,
{
    pub fn new(payload: T) -> Self {
        Self {
            payload,
            #[cfg(feature = "formal-trace")]
            id: super::formal_trace::id(),
        }
    }

    /// Consume the messages into the payload
    pub fn into_payload(self) -> T {
        self.payload
    }
}

/// A connector to the shared bus
pub struct BusConnector<T>
where
    T: Clone,
{
    state: SharedState<T>,
    receiver: Receiver<Message<T>>,
    #[cfg(feature = "formal-trace")]
    receiver_id: u64,
}

impl<T: Debug + Clone> Debug for BusConnector<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BusConnection {{ sender: {:?}, receiver: {:?} }}",
            self.state.sender, self.receiver
        )
    }
}

#[cfg(feature = "formal-trace")]
impl<T: Clone> Drop for BusConnector<T> {
    fn drop(&mut self) {
        super::formal_trace::record("leave", serde_json::json!({"receiver": self.receiver_id}));
    }
}

impl<T> BusConnector<T>
where
    T: Clone,
{
    fn new(
        receiver: Receiver<Message<T>>,
        state: SharedState<T>,
        #[cfg(feature = "formal-trace")] receiver_id: u64,
    ) -> Self {
        Self {
            receiver,
            state,
            #[cfg(feature = "formal-trace")]
            receiver_id,
        }
    }

    /// Add another connection to the bus
    pub fn join_bus(&self) -> BusConnector<T> {
        self.state.join_bus()
    }

    fn wrap(&self, payload: T) -> Message<T> {
        Message::new(payload)
    }

    /// Blocking
    #[cfg(test)]
    pub fn get_n_events_blocking(&self, quantity: usize) -> Vec<T> {
        self.receiver
            .iter()
            .map(|m| m.into_payload())
            .take(quantity)
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.receiver
            .iter()
            .map(|message| self.unwrap_message(message))
    }

    /// Non blocking
    /// Todo: rename to be generic.
    pub fn try_get_events(&self) -> Vec<T> {
        self.receiver
            .try_iter()
            .map(|m| self.unwrap_message(m))
            .collect()
    }

    fn unwrap_message(&self, message: Message<T>) -> T {
        #[cfg(feature = "formal-trace")]
        {
            super::formal_trace::record(
                "receive",
                serde_json::json!({
                    "message": message.id, "receiver": self.receiver_id,
                }),
            );
            super::formal_trace::set_cause(message.id);
        }
        message.into_payload()
    }

    #[cfg(feature = "formal-trace")]
    pub(crate) fn trace_receiver(&self) -> u64 {
        self.receiver_id
    }

    #[cfg(feature = "formal-trace")]
    pub(crate) fn try_get_events_with_ids(&self) -> Vec<(u64, T)> {
        self.receiver
            .try_iter()
            .map(|message| {
                let id = message.id;
                (id, self.unwrap_message(message))
            })
            .collect()
    }

    #[cfg(feature = "formal-trace")]
    pub(crate) fn send_recorded_event(&self, ev: T, metadata: serde_json::Value) {
        let message = self.wrap(ev);
        super::formal_trace::record(
            "send",
            serde_json::json!({
                "message": message.id, "event": metadata, "sender": self.receiver_id,
            }),
        );
        self.state
            .sender
            .send(message)
            .expect("Failed sending update event!");
    }

    pub fn send_event(&self, ev: T) {
        let message = self.wrap(ev);
        #[cfg(feature = "formal-trace")]
        super::formal_trace::record(
            "send",
            serde_json::json!({
                "message": message.id, "event": null, "sender": self.receiver_id,
            }),
        );
        self.state
            .sender
            .send(message)
            .expect("Failed sending update event!");
    }
}

#[cfg(test)]
mod test {
    use std::thread;
    use std::time::Duration;

    use crossbeam_channel as channel;

    use crate::horust::bus::{Bus, BusConnector};
    //TODO: remove this reference:
    use crate::horust::formats::{Event, ServiceStatus, ShuttingDown};

    fn init_bus() -> (
        BusConnector<Event>,
        BusConnector<Event>,
        channel::Receiver<()>,
    ) {
        let bus = Bus::new();
        let a = bus.join_bus();
        let b = bus.join_bus();
        let (sender, receiver) = channel::bounded(48);

        let _handle = thread::spawn(move || {
            bus.run();
            sender
                .send(())
                .expect("test didn't terminate in time, so chan is closed!");
        });
        (a, b, receiver)
    }

    #[test]
    // tests get_events function both blocking and non-blocking
    fn test_get_events() {
        let (a, b, receiver_a) = init_bus();
        let ev = Event::new_status_changed("sample", ServiceStatus::Initial);
        let (sender, receiver_b) = channel::bounded(48);

        let _handle = thread::spawn(move || {
            a.send_event(ev.clone());
            a.send_event(ev.clone());
            a.send_event(ev.clone());
            a.get_n_events_blocking(3);
            b.get_n_events_blocking(3);

            a.send_event(ev.clone());
            while a.receiver.is_empty() && b.receiver.is_empty() {
                thread::sleep(Duration::from_millis(200));
            }
            assert_eq!(a.try_get_events().len(), 1);
            assert_eq!(b.try_get_events().len(), 1);
            sender.send(()).unwrap();
        });

        receiver_a
            .recv_timeout(Duration::from_secs(3))
            .expect("Didn't receive an answer on time.");

        receiver_b
            .recv_timeout(Duration::from_secs(3))
            .expect("Didn't receive an answer on time.");
    }

    #[test]
    fn test_bus_simple() {
        let (a, b, receiver) = init_bus();
        let ev = Event::new_status_changed("sample", ServiceStatus::Initial);
        a.send_event(ev.clone());
        assert_eq!(a.receiver.recv().unwrap().into_payload(), ev);
        assert_eq!(b.receiver.recv().unwrap().into_payload(), ev);
        drop(a);
        drop(b);
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("Didn't receive an answer on time.");
    }

    #[test]
    fn test_bus_nested() {
        let (a, b, receiver) = init_bus();
        let c = a.join_bus();

        let ev = Event::new_status_changed("sample", ServiceStatus::Initial);
        a.send_event(ev.clone());
        assert_eq!(a.receiver.recv().unwrap().into_payload(), ev);
        assert_eq!(b.receiver.recv().unwrap().into_payload(), ev);
        assert_eq!(c.receiver.recv().unwrap().into_payload(), ev);
        drop(a);
        drop(b);
        drop(c);
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("Didn't receive an answer on time.");
    }

    #[test]
    fn test_stress() {
        let bus = Bus::new();
        let mut connectors = vec![];
        let last = bus.join_bus();
        for _i in 0..100 {
            connectors.push(bus.join_bus());
        }
        let (sender, receiver) = channel::bounded(48);
        let _handle = thread::spawn(move || {
            bus.run();
            sender
                .send(())
                .expect("test didn't terminate in time, so chan is closed!");
        });
        let ev = Event::new_status_changed("sample", ServiceStatus::Initial);
        let exit_ev = Event::ShuttingDownInitiated(ShuttingDown::Gracefully);

        for _i in 0..100 {
            last.send_event(ev.clone());
            for recv in &connectors {
                assert_eq!(recv.receiver.recv().unwrap().into_payload(), ev);
            }
            let to_stop = connectors.pop().unwrap();
            to_stop.send_event(exit_ev.clone());
            for recv in &connectors {
                assert_eq!(recv.receiver.recv().unwrap().into_payload(), exit_ev);
            }
        }
        last.send_event(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
        drop(connectors);
        drop(last);
        receiver
            .recv_timeout(Duration::from_secs(15))
            .expect("Didn't receive an answer on time.");
    }
}
