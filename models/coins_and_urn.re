heads_1 <- Probability(0.6).
heads_2 <- Probability(0.4).

blue <- Probability(0.1).

win if heads_1 and heads_2.
win if blue.

win -> target("/winning").