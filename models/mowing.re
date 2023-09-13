/* Source signals 
 * Received from sensors, MLPs, estimators, ...
 */
raining <- source("/rain_sensor/raining", Density).
day <- source("/calendar", Number).

/* First-order logic */
thunder <- P(0.1).
sunny <- P(0.8).
grass_long(l1) <- P(0.7).
grass_long(l2) <- P(0.25).

lawn(l1).
lawn(l2).

cloudy if not sunny.
mow_grass(L) if lawn(L) and not raining and grass_long(L).
noisy if mow_grass(L) and lawn(L).
noisy if thunder.

/* Target signals 
 * Computed via probabilistic inference in Reactive Circuit
 */
noisy -> target("/noise").
