use rclrs::{Node, Publisher, QoSHistoryPolicy, RclrsError, Subscription, QOS_PROFILE_DEFAULT};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use std_msgs::msg::Float64;

use crate::circuit::leaf::{update, Foliage};
use crate::circuit::reactive::RcQueue;

#[derive(Clone)]
pub struct IpcReader {
    pub topic: String,
    subscription: Arc<Subscription<Float64>>,
}

pub struct IpcWriter {
    pub frequency: f64,
    publisher: Arc<Publisher<Float64>>,
    value: Arc<Mutex<f64>>,
    sender: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl IpcReader {
    pub fn new(
        node: &mut Node,
        foliage: Foliage,
        rc_queue: RcQueue,
        index: u16,
        channel: &str,
        invert: bool,
    ) -> Result<Self, RclrsError> {
        let mut prefix = "";
        // TODO: Remove prefix, only send on one topic but invert for negated leaf
        if invert {
            prefix = "/not";
        }

        let mut profile = QOS_PROFILE_DEFAULT;
        profile.history = QoSHistoryPolicy::KeepLast { depth: 1 };

        let subscription = node.create_subscription(
            &format!("{}{}", prefix, channel),
            profile,
            move |msg: Float64| {
                let value = if invert { 1.0 - msg.data } else { msg.data };

                update(&foliage, &rc_queue, index, value);
            },
        )?;

        Ok(Self {
            topic: format!("{}{}", prefix, channel),
            subscription,
        })
    }
}

impl IpcWriter {
    pub fn new(
        node: &Node,
        topic: &str,
        frequency: f64,
    ) -> Result<Self, RclrsError> {
        let mut profile = QOS_PROFILE_DEFAULT;
        profile.history = QoSHistoryPolicy::KeepLast { depth: 1 };

        let publisher = Arc::new(node.create_publisher(topic, profile)?);

        let value = Arc::new(Mutex::new(0.0));

        Ok(Self {
            frequency,
            publisher,
            value,
            sender: None,
            handle: None,
        })
    }

    pub fn get_value_access(&self) -> Arc<Mutex<f64>> {
        self.value.clone()
    }

    pub fn start(&mut self) {
        use std::thread::spawn;

        // If this is already running, we don't do anything
        if self.sender.is_some() {
            return;
        }

        // Make copies such that self isn't moved here
        let thread_publisher = self.publisher.clone();
        let thread_value = self.value.clone();
        let thread_timeout = Duration::from_secs_f64(1.0 / self.frequency);

        // Create a channel to later terminate the thread
        let (sender, receiver) = mpsc::channel();
        self.sender = Some(sender);

        let clock = Instant::now();
        self.handle = Some(spawn(move || loop {
            // Publish next value
            let _ = thread_publisher.publish(Float64 {
                data: *thread_value.lock().unwrap(),
            });

            // Break if notified via channel or disconnected
            match receiver.recv_timeout(thread_timeout) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => (),
            }
        }));
    }

    pub fn stop(&mut self) {
        if self.sender.is_some() {
            // Reset members to None
            let sender = self.sender.take();
            let handle = self.handle.take();

            // Send message and wait for join
            let _ = sender.unwrap().send(());
            handle
                .unwrap()
                .join()
                .expect("Could not join with writer thread!");
        }
    }
}

impl Drop for IpcWriter {
    fn drop(&mut self) {
        self.stop();
    }
}
