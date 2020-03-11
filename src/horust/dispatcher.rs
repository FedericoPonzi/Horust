use crate::horust::service_handler::{Event, EventKind, ServiceHandler};
use crossbeam::channel::{unbounded, Receiver, Sender};

/// Since I couldn't find any satisfying crate for broadcasting messages,
/// I'm using this struct for distributing the messages among the queues - read: bus.
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

    // Will use the thread for running the dispatcher.
    pub fn run(mut self) {
        self.dispatch();
    }

    // Add another component to the bus
    pub fn join_bus(&mut self) -> BusConnector {
        let (mysx, rx) = unbounded();
        self.senders.push(mysx);
        BusConnector::new(self.public_sender.clone(), rx)
    }

    // Infinite dispatching loop.
    // TODO: handle error or try_send or send_timeout
    pub fn dispatch(&mut self) {
        loop {
            self.receiver.iter().for_each(|el| {
                self.senders
                    .iter()
                    .for_each(|sender| sender.send(el.clone()).unwrap())
            });
        }
    }
}

#[derive(Debug, Clone)]
pub struct BusConnector {
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}
impl BusConnector {
    pub fn new(sender: Sender<Event>, receiver: Receiver<Event>) -> Self {
        BusConnector { sender, receiver }
    }

    fn send_update(&self, ev: Event) {
        //debug!("Going to send the following event: {:?}", ev);
        self.sender.send(ev).expect("Failed sending update event!");
    }

    pub fn try_get_events(&self) -> Vec<Event> {
        self.receiver.try_iter().collect()
    }

    pub fn send_update_pid(&self, sh: &ServiceHandler) {
        self.send_update(Event::new(sh.clone(), EventKind::PidChanged));
    }

    pub fn send_updated_status(&self, sh: &ServiceHandler) {
        self.send_update(Event::new(sh.clone(), EventKind::StatusChanged));
    }
}
