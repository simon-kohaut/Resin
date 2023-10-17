use lazy_static::lazy_static;
use rclrs::{spin_once, Context, Node, RclrsError, Subscription, QOS_PROFILE_DEFAULT};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std_msgs::msg::Float64;

use super::leaf::update;
use super::SharedLeaf;

lazy_static! {
    static ref CONTEXT: Context = Context::new(vec![]).unwrap();
    static ref NODE: Arc<Mutex<Node>> =
        Arc::new(Mutex::new(Node::new(&CONTEXT, "resin_ipc").unwrap()));
}

pub struct IpcChannel {
    subscription: Arc<Subscription<Float64>>,
}

pub fn retreive_messages() {
    let _ = spin_once(&NODE.lock().unwrap(), Some(Duration::from_millis(1)));
}

impl IpcChannel {
    pub fn new(leaf: SharedLeaf, channel: String, invert: bool) -> Result<Self, RclrsError> {
        let mut prefix = "";
        if invert {
            prefix = "/not";
        }

        let subscription = NODE.lock().unwrap().create_subscription(
            &format!("{}{}", prefix, channel),
            QOS_PROFILE_DEFAULT,
            move |msg: Float64| {
                if invert {
                    update(&leaf, &(1.0 - msg.data));
                } else {
                    update(&leaf, &msg.data);
                }
            },
        )?;

        Ok(Self { subscription })
    }
}
