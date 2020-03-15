use crate::horust::formats::{Event, EventKind, ServiceHandler, ServiceName};
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
                    .for_each(|sender| sender.send(el.clone()).expect("Failed sending message"))
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

    /// Non blocking
    pub fn try_get_events(&self) -> Vec<Event> {
        self.receiver.try_iter().collect()
    }

    fn send_update(&self, ev: Event) {
        //debug!("Going to send the following event: {:?}", ev);
        self.sender.send(ev).expect("Failed sending update event!");
    }

    pub fn send_exit_code(&self, service_name: ServiceName, exit_code: i32) {
        self.send_update(Event::new(
            service_name,
            EventKind::ServiceExited(exit_code),
        ));
    }

    pub fn send_updated_pid(&self, sh: &ServiceHandler) {
        self.send_update(Event::new(
            sh.name().clone(),
            EventKind::PidChanged(sh.pid.unwrap()),
        ));
    }

    pub fn send_updated_status(&self, sh: &ServiceHandler) {
        self.send_update(Event::new(
            sh.service().name.clone(),
            EventKind::StatusChanged(sh.status.clone()),
        ));
    }

    pub fn send_updated_marked_for_killing(&self, sh: &ServiceHandler) {
        self.send_update(Event::new(
            sh.name().clone(),
            EventKind::MarkedForKillingChanged(sh.marked_for_killing.clone()),
        ));
    }
}
