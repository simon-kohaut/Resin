/* Source signals 
 * Received from sensors, MLPs, estimators, ...
 */
raining <- source("/rain_sensor/raining", Density).
day <- source("/calendar", Number).

/* First-order logic */
sunny <- Probability(0.8).
cloudy if not sunny.
mow_grass(L) <- Probability(0.3) if lawn(L) and not raining and grass_long(L).
mow_grass(L) <- Probability(0.3) if lawn(L) and day and sunny and grass_long(L).

lawn(l1).
lawn(l2).

grass_long(l1) <- Probability(0.7).
grass_long(l2) <- Probability(0.25).

noisy if mow_grass(l1) and lawn(l1).
noisy if mow_grass(l2) and lawn(l2).

/* Target signals 
 * Computed via probabilistic inference in Reactive Circuit
 */
noisy -> target("/noise").
