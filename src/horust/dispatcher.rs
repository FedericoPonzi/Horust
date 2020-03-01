use crate::horust::service_handler::{Event, UpdatesQueue};
use crossbeam_channel::{unbounded, Receiver, Sender};

/// Since I couldn't find any statisfying crate for broadcasting messages,
/// I'm using this struct for distributing the messages among the queues - read: bus.
#[derive(Clone, Debug)]
pub struct Dispatcher {
    public_sender: Sender<Event>,
    receiver: Receiver<Event>,
    senders: Vec<Sender<Event>>,
}

impl Dispatcher {
    pub fn new() -> Self {
        let (pub_sx, rx) = unbounded();
        Dispatcher {
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
    pub fn add_component(&mut self) -> UpdatesQueue {
        let (mysx, rx) = unbounded();
        self.senders.push(mysx);
        UpdatesQueue::new(self.public_sender.clone(), rx)
    }
    // Infinite dispatching loop.
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
