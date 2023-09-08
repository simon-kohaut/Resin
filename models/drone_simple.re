rain <- Probability(0.3).
clearance <- Probability(0.8).
speed <- Probability(0.5).

safe <- Probability(0.9) if clearance and not rain and speed.
safe if clearance and not rain and not speed.
safe if clearance and rain and not speed.

safe -> target("/safety").
