use crate::horust::formats::Event;
use crossbeam::channel::{unbounded, Receiver, Sender};

/// A simple bus implementation: distributes the messages among the queues
#[derive(Debug)]
pub struct Bus {
    public_sender: Sender<Event>,
    receiver: Receiver<Event>,
    senders: Vec<Sender<Event>>,
}

impl Bus {
    pub fn new() -> Self {
        let (pub_sx, rx) = unbounded();
        Bus {
            public_sender: pub_sx,
            receiver: rx,
            senders: Vec::new(),
        }
    }

    /// Blocking
    pub fn run(self) {
        self.dispatch();
    }

    /// Add another connection to the bus
    pub fn join_bus(&mut self) -> BusConnector {
        let (mysx, rx) = unbounded();
        self.senders.push(mysx);
        BusConnector::new(self.public_sender.clone(), rx)
    }

    /// Dispatching loop
    /// As soon as we don't have any senders it will exit
    pub fn dispatch(mut self) {
        for ev in self.receiver {
            debug!("Received ev: {:?}", ev);
            debug!("self.senders: {:?}", self.senders.len());
            self.senders
                .retain(|sender| sender.send(ev.clone()).is_ok());
            if self.senders.is_empty() {
                break;
            }
        }
    }
}

/// A connector to the shared bus
#[derive(Debug, Clone)]
pub struct BusConnector {
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}
impl BusConnector {
    pub fn new(sender: Sender<Event>, receiver: Receiver<Event>) -> Self {
        BusConnector { sender, receiver }
    }

    /// Blocking
    pub fn get_events_blocking(&self) -> Event {
        self.receiver.recv().unwrap()
    }

    /// Blocking
    pub fn get_n_events_blocking(&self, quantity: usize) -> Vec<Event> {
        self.receiver.iter().take(quantity).collect()
    }

    /// Non blocking
    pub fn try_get_events(&self) -> Vec<Event> {
        self.receiver.try_iter().collect()
    }

    pub(crate) fn send_event(&self, ev: Event) {
        self.sender.send(ev).expect("Failed sending update event!");
    }
}
