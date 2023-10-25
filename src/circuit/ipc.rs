use lazy_static::lazy_static;
use rclrs::{spin_once, Context, Node, Publisher, RclrsError, Subscription, QOS_PROFILE_DEFAULT};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std_msgs::msg::Float64;

use super::leaf::{update, Foliage};

lazy_static! {
    static ref CONTEXT: Context = Context::new(vec![]).unwrap();
    static ref NODE: Arc<Mutex<Node>> =
        Arc::new(Mutex::new(Node::new(&CONTEXT, "resin_ipc").unwrap()));
}

#[derive(Clone)]
pub struct IpcChannel {
    pub topic: String,
    subscription: Arc<Subscription<Float64>>,
}

pub struct RandomizedIpcChannel {
    pub frequency: f64,
    publisher: Publisher<Float64>,
    value: f64,
}

pub fn retreive_messages() {
    let _ = spin_once(&NODE.lock().unwrap(), Some(Duration::from_millis(1)));
}

pub fn shutdown() {
    drop(&NODE);
}

impl IpcChannel {
    pub fn new(
        foliage: Foliage,
        index: usize,
        channel: String,
        invert: bool,
    ) -> Result<Self, RclrsError> {
        let mut prefix = "";
        // TODO: Remove prefix, only send on one topic but invert for negated leaf
        if invert {
            prefix = "/not";
        }

        let subscription = NODE.lock().unwrap().create_subscription(
            &format!("{}{}", prefix, channel),
            QOS_PROFILE_DEFAULT,
            move |msg: Float64| {
                if invert {
                    update(foliage.clone(), index, &(1.0 - msg.data));
                } else {
                    update(foliage.clone(), index, &msg.data);
                }
            },
        )?;

        Ok(Self {
            topic: format!("{}{}", prefix, channel),
            subscription,
        })
    }
}

impl RandomizedIpcChannel {
    pub fn new(topic: &str, frequency: f64, value: f64) -> Result<Self, RclrsError> {
        let publisher = NODE
            .lock()
            .unwrap()
            .create_publisher(topic, QOS_PROFILE_DEFAULT)?;

        Ok(Self {
            frequency,
            publisher,
            value,
        })
    }

    pub fn start(self) {
        std::thread::spawn(move || -> Result<(), rclrs::RclrsError> {
            loop {
                if !CONTEXT.ok() {
                    return Ok(());
                }

                std::thread::sleep(Duration::from_secs_f64(1.0 / self.frequency));
                self.publisher.publish(Float64 { data: self.value })?;
            }
        });
    }
}
