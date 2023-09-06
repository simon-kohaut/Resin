high_speed <- source("/imu", Probability).
rain <- source("/rain", Density).

safe if not rain.
safe if rain and not high_speed.

safe -> target("/safety").
