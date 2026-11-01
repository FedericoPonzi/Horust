--------------------------- MODULE SupervisorMC ---------------------------
EXTENDS Naturals
VARIABLES phase, st, owned, active, blocked
Deps(s) == IF s = 1 THEN {} ELSE {1}
Stops(s) == IF s = 1 THEN {2} ELSE {}
Kills(s) == {t \in {1, 2}: s \in Deps(t)}
INSTANCE Supervisor WITH Services <- {1, 2}, StartDeps <- Deps, StopDeps <- Stops,
    KillTargets <- Kills, FailurePolicy <- "KillDependents", ExternalShutdown <- TRUE
=============================================================================
