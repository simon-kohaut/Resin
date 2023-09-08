/* Source signals 
 * Received from sensors, MLPs, estimators, ...
 */
raining <- source("/rain_sensor/raining", Density).
day <- source("/calendar", Number).

/* First-order logic */
thunder <- Probability(0.1).
sunny <- Probability(0.8).
cloudy if not sunny.
mow_grass(L) <- Probability(0.3) if lawn(L) and not raining and grass_long(L).
mow_grass(L) <- Probability(0.3) if lawn(L) and day and sunny and grass_long(L).

lawn(l1).
lawn(l2).

grass_long(l1) <- Probability(0.7).
grass_long(l2) <- Probability(0.25).

noisy if mow_grass(L) and lawn(L).
noisy if thunder.

/* Target signals 
 * Computed via probabilistic inference in Reactive Circuit
 */
noisy -> target("/noise").
