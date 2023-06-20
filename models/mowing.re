/* Source signals 
 * Received from sensors, MLPs and estimators
 */
raining <- source(
    "/rain_sensor", 
    Probability(2)
).
day <- source(
    "/calendar", 
    Number
).

// First-order logic
grass_long <- Probability(0.7).
mow_grass if day != 7 and
    not raining and
    grass_long.

/*
 * Target signals 
 * Computed via probabilistic inference in Reactive Circuit
 */
mow_grass -> target(
    "/start_mowing"
).
