burglary <- P(0.1).
hears_alarm(mary) <- P(0.5).
hears_alarm(john) <- P(0.4).
earthquake <- P(0.2).

alarm_e <- P(0.1) if earthquake.
alarm_b <- P(0.5) if burglary.

alarm if alarm_e.
alarm if alarm_b.

calls(mary) if alarm and hears_alarm(mary).

calls(mary) -> target("/landline").
