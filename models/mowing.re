/* Source signals 
 * Received from sensors, MLPs, estimators, ...
 */
raining <- source("/rain_sensor", Probability).
day <- source("/calendar", Number).

/* First-order logic
 */
grass_long <- Probability(0.7).
sunny <- Probability(0.8).

cloudy if not sunny.
mow_grass if day != 7 and not raining and sunny and grass_long.

/*
 * Target signals 
 * Computed via probabilistic inference in Reactive Circuit
 */
mow_grass -> target("/start_mowing").
