rain <- source("/rain", Probability).
clearance <- source("/lidar/distance", Density).
speed <- source("/imu/speed", Number).

safe if clearance and not rain and speed.
safe if clearance and not rain and not speed.
safe if clearance and rain and not speed.

safe -> target("/safety").
