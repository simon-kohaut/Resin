use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rclrs::{Node, Publisher, QoSHistoryPolicy, RclrsError, Subscription, QOS_PROFILE_DEFAULT};
use std_msgs::msg::Float64MultiArray;

use crate::circuit::leaf::{update, Foliage};
use crate::circuit::reactive::RcQueue;

use super::Vector;

#[derive(Clone)]
pub struct IpcReader {
    pub topic: String,
    subscription: Arc<Subscription<Float64MultiArray>>,
}

pub struct IpcWriter {
    publisher: Arc<Publisher<Float64MultiArray>>,
}

pub struct TimedIpcWriter {
    pub frequency: f64,
    publisher: Arc<Publisher<Float64MultiArray>>,
    value: Arc<Mutex<f64>>,
    sender: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl IpcReader {
    pub fn new(
        node: Arc<Node>,
        foliage: Foliage,
        rc_queue: RcQueue,
        index: u16,
        channel: &str,
        invert: bool,
    ) -> Result<Self, RclrsError> {
        let mut profile = QOS_PROFILE_DEFAULT;
        profile.history = QoSHistoryPolicy::KeepLast { depth: 1 };

        let subscription =
            node.create_subscription(channel, profile, move |msg: Float64MultiArray| {
                let data = Vector::from_iter(msg.data.clone().into_iter().skip(1));
                let value = if invert { 1.0 - data } else { data };

                update(&foliage, &rc_queue, index, value, msg.data[0]);
            })?;

        Ok(Self {
            topic: channel.to_owned(),
            subscription,
        })
    }
}

impl IpcWriter {
    pub fn new(node: Arc<Node>, topic: &str) -> Result<Self, RclrsError> {
        let mut profile = QOS_PROFILE_DEFAULT;
        profile.history = QoSHistoryPolicy::KeepLast { depth: 1 };

        let publisher = node.create_publisher(topic, profile)?;

        Ok(Self { publisher })
    }

    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        let timestamp = if timestamp.is_none() {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring UNIX timestamp failed!")
                .as_secs_f64()
        } else {
            timestamp.unwrap()
        };

        let mut message = Float64MultiArray::default();
        message.data = vec![timestamp];
        message.data.append(&mut value.to_vec());

        // Publish next value
        let _ = self.publisher.publish(message);
    }
}

impl TimedIpcWriter {
    pub fn new(node: Arc<Node>, topic: &str, frequency: f64) -> Result<Self, RclrsError> {
        let mut profile = QOS_PROFILE_DEFAULT;
        profile.history = QoSHistoryPolicy::KeepLast { depth: 1 };

        let publisher = node.create_publisher(topic, profile)?;
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

        self.handle = Some(spawn(move || loop {
            let value = *thread_value.lock().unwrap();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring timestamp failed!")
                .as_secs_f64();

            let mut message = Float64MultiArray::default();
            message.data = vec![value, timestamp];

            // Publish next value
            let _ = thread_publisher.publish(message);

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

impl Drop for TimedIpcWriter {
    fn drop(&mut self) {
        self.stop();
    }
}
