use crossbeam::channel::{unbounded, Receiver, Sender};

/// A simple bus implementation: distributes the messages among the queues
/// There is one single input pipe (`public_sender` ; `receiver`). The sender side is shared among
/// all the publishers. The bus reads from the receiver, and publishes to all the `senders`.
#[derive(Debug)]
pub struct Bus<T> {
    /// Bus input - sender side
    public_sender: Sender<T>,
    /// Bus input - receiver side
    receiver: Receiver<T>,
    /// Bus output - all the senders
    senders: Vec<Sender<T>>,
}

impl<T> Bus<T>
where
    T: Clone + std::fmt::Debug,
{
    pub fn new() -> Self {
        let (public_sender, receiver) = unbounded();
        Bus {
            public_sender,
            receiver,
            senders: Default::default(),
        }
    }

    /// Blocking
    pub fn run(self) {
        self.dispatch();
    }

    /// Add another connection to the bus
    pub fn join_bus(&mut self) -> BusConnector<T> {
        let (sender, receiver) = unbounded();
        self.senders.push(sender);
        BusConnector::new(self.public_sender.clone(), receiver)
    }

    /// Dispatching loop
    /// As soon as we don't have any senders it will exit
    fn dispatch(mut self) {
        drop(self.public_sender);
        for ev in self.receiver {
            self.senders
                .retain(|sender| sender.send(ev.clone()).is_ok());
        }
    }
}

/// A connector to the shared bus
#[derive(Debug, Clone)]
pub struct BusConnector<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
}
impl<T> BusConnector<T> {
    pub fn new(sender: Sender<T>, receiver: Receiver<T>) -> Self {
        BusConnector { sender, receiver }
    }

    /// Blocking
    pub fn get_n_events_blocking(&self, quantity: usize) -> Vec<T> {
        self.receiver.iter().take(quantity).collect()
    }
    pub fn iter(&self) -> crossbeam::Iter<T> {
        self.receiver.iter()
    }
    /// Non blocking
    pub fn try_get_events(&self) -> Vec<T> {
        self.receiver.try_iter().collect()
    }

    pub(crate) fn send_event(&self, ev: T) {
        self.sender.send(ev).expect("Failed sending update event!");
    }
}

#[cfg(test)]
mod test {
    use crate::horust::bus::{Bus, BusConnector};
    use crate::horust::formats::{Event, ServiceStatus};
    use crossbeam::channel;
    use std::thread;
    use std::time::Duration;

    fn init_bus() -> (
        BusConnector<Event>,
        BusConnector<Event>,
        channel::Receiver<()>,
    ) {
        let mut bus = Bus::new();
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
        let ev = Event::new_status_changed(&"sample".to_string(), ServiceStatus::Initial);
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

            a.send_event(Event::new_exit_success("serv"));
            b.send_event(Event::new_exit_success("serv"));
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
        let ev = Event::new_status_changed(&"sample".to_string(), ServiceStatus::Initial);
        a.send_event(ev.clone());
        assert_eq!(a.receiver.recv().unwrap(), ev);
        assert_eq!(b.receiver.recv().unwrap(), ev);
        a.send_event(Event::new_exit_success("serv"));
        b.send_event(Event::new_exit_success("serv"));
        drop(a);
        drop(b);
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("Didn't receive an answer on time.");
    }

    #[test]
    fn test_stress() {
        let mut bus = Bus::new();
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
        let ev = Event::new_status_changed(&"sample".to_string(), ServiceStatus::Initial);
        let exit_ev = Event::new_exit_success("serv");

        for _i in 0..100 {
            last.send_event(ev.clone());
            for recv in &connectors {
                assert_eq!(recv.receiver.recv().unwrap(), ev);
            }
            let to_stop = connectors.pop().unwrap();
            to_stop.send_event(exit_ev.clone());
            for recv in &connectors {
                assert_eq!(recv.receiver.recv().unwrap(), exit_ev);
            }
        }
        last.send_event(Event::new_exit_success("serv"));
        drop(connectors);
        drop(last);
        receiver
            .recv_timeout(Duration::from_secs(15))
            .expect("Didn't receive an answer on time.");
    }
}
