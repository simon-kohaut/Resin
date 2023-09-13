heads_1 <- P(0.5).
heads_2 <- P(0.5).
win_1 <- P(0.7) if heads_1.
win_2 <- P(0.7) if heads_2.

win if win_1.
win if win_2.

win -> target("/winning").