/* Source signals 
 * Received from sensors, MLPs, estimators, ...
 */
raining <- source("/rain_sensor", Density).
day <- source("/calendar", Number).

/* First-order logic */
grass_long <- Probability(0.7).
sunny <- Probability(0.8).
cloudy if not sunny.
mow_grass <- Probability(0.3) if raining < 0.25 and day != 7 and sunny and grass_long.

/* Target signals 
 * Computed via probabilistic inference in Reactive Circuit
 */
mow_grass -> target("/start_mowing").
