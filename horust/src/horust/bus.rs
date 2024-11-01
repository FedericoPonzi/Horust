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

use crossbeam::channel::{unbounded, Receiver, Sender};

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
    senders: Arc<Mutex<Vec<Sender<Message<T>>>>>,
}

impl<T> SharedState<T>
where
    T: Clone,
{
    /// Add another connection to the bus
    pub fn join_bus(&self) -> BusConnector<T> {
        let mut senders = self.senders.lock().unwrap();

        let (sender, receiver) = unbounded();
        senders.push(sender);

        BusConnector::new(receiver, self.clone())
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

impl<T> Bus<T>
where
    T: Clone,
{
    pub fn new() -> Self {
        let (public_sender, receiver) = unbounded();
        Bus {
            state: SharedState {
                sender: public_sender,
                senders: Default::default(),
            },
            receiver,
        }
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
            let mut senders = self.state.senders.lock().unwrap();
            senders.retain(|sender| sender.send(ev.clone()).is_ok());
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
}

impl<T> Message<T>
where
    T: Clone,
{
    pub fn new(payload: T) -> Self {
        Self { payload }
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

impl<T> BusConnector<T>
where
    T: Clone,
{
    fn new(receiver: Receiver<Message<T>>, state: SharedState<T>) -> Self {
        Self { receiver, state }
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
        self.receiver.iter().map(|message| message.into_payload())
    }

    /// Non blocking
    /// Todo: rename to be generic.
    pub fn try_get_events(&self) -> Vec<T> {
        self.receiver.try_iter().map(|m| m.into_payload()).collect()
    }

    pub(crate) fn send_event(&self, ev: T) {
        self.state
            .sender
            .send(self.wrap(ev))
            .expect("Failed sending update event!");
    }
}

#[cfg(test)]
mod test {
    use std::thread;
    use std::time::Duration;

    use crossbeam::channel;

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
