use rclrs::{Node, Publisher, QoSHistoryPolicy, RclrsError, Subscription, QOS_PROFILE_DEFAULT};
use std::sync::Arc;
use std::time::Duration;
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
    value: f64,
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
    pub fn new(node: &Node, topic: &str, frequency: f64, value: f64) -> Result<Self, RclrsError> {
        let mut profile = QOS_PROFILE_DEFAULT;
        profile.history = QoSHistoryPolicy::KeepLast { depth: 1 };

        let publisher = Arc::new(node.create_publisher(topic, profile)?);

        Ok(Self {
            frequency,
            publisher,
            value,
        })
    }

    pub fn start(&self) {
        let thread_publisher = self.publisher.clone();
        let thread_value = self.value;
        let thread_frequency = self.frequency;

        std::thread::spawn(move || loop {
            match thread_publisher.publish(Float64 { data: thread_value }) {
                Ok(_) => (),
                Err(_) => break,
            }
            std::thread::sleep(Duration::from_secs_f64(1.0 / thread_frequency));
        });
    }
}
