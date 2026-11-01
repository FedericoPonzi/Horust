--------------------------- MODULE ProcessRules ---------------------------
EXTENDS Integers, Sequences

Terminal == {"Finished", "FinishedFailed"}
Early == {"Initial", "Starting", "Started"}
Live == {"Starting", "Started", "Running"}
Statuses == Early \cup Terminal \cup {"Running", "InKilling", "Success", "Failed"}
EmptyProcess == [status |-> "Initial", pid |-> 0, retries |-> 0, hc |-> -1,
                 timer |-> FALSE]
HasFailed(p) == p.hc # 0
Exceeded(p, c) == p.hc > c.max_failed
Allowed(target) ==
    CASE target = "Initial" -> {"Success", "Failed"}
      [] target = "Starting" -> {"Initial"}
      [] target = "Started" -> {"Starting"}
      [] target = "Running" -> {"Started"}
      [] target = "InKilling" -> {"Initial", "Starting", "Started", "Running"}
      [] target = "FinishedFailed" -> {"Starting", "Started", "Failed", "InKilling"}
      [] target \in {"Success", "Failed"} -> {"Starting", "Started", "Running", "InKilling"}
      [] target = "Finished" -> {"Success", "Initial"}
      [] OTHER -> {}

Update(p, target, c) ==
    IF p.status = target \/ p.status \notin Allowed(target) THEN p
    ELSE [p EXCEPT
        !.status = IF target = "InKilling" /\ p.status = "Initial"
                   THEN "Success" ELSE target,
        !.retries = IF target = "Running" THEN 0
                    ELSE IF target = "InKilling" /\ p.status = "Started" /\ Exceeded(p, c)
                    THEN @ + 1 ELSE @]

RestartTarget(p, c) ==
    LET over == c.attempts = 0 \/ p.retries > c.attempts
        bounded == c.attempts > 0 /\ over
        stop == IF c.strategy = "Never" THEN p.retries = 0 \/ over ELSE bounded
    IN IF p.status = "Failed"
       THEN IF stop THEN "FinishedFailed" ELSE "Initial"
       ELSE IF c.strategy = "Always" /\ ~bounded THEN "Initial" ELSE "Finished"

ExitProcess(p, c, successful) ==
    LET failed == ~successful \/ Exceeded(p, c) \/ (p.status = "Running" /\ HasFailed(p))
        target == IF failed THEN "Failed" ELSE "Success"
        counted == [p EXCEPT !.retries = @ + IF HasFailed(p) /\ p.status \in Early
                                                    THEN 1 ELSE 0,
                             !.pid = 0, !.timer = FALSE]
    IN Update(counted, target, c)

HealthProcess(p, good) ==
    [p EXCEPT !.hc = IF p.status \in Live
                    THEN IF good THEN 0 ELSE IF @ < 0 THEN 1
                         ELSE IF @ = 2147483647 THEN @ ELSE @ + 1
                    ELSE IF @ < 0 THEN 0 ELSE @]

WellTyped(p) ==
    /\ p.status \in Statuses /\ p.pid \in Nat /\ p.retries \in Nat
    /\ p.hc \in Int /\ p.hc >= -1 /\ p.timer \in BOOLEAN
=============================================================================
