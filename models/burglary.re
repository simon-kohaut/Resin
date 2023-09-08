burglary <- Probability(0.1).
hears_alarm(mary) <- Probability(0.5).
hears_alarm(john) <- Probability(0.4).
earthquake <- Probability(0.2).

alarm if earthquake.
alarm if burglary.

calls(X) if alarm and hears_alarm(X).

calls(mary) -> target("/landline").
