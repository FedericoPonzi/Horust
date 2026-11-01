------------------------- MODULE ProcessExtended -------------------------
EXTENDS ProcessRules, TLC
CONSTANT Strategy, Budget, MaxFailed, Scenario, TermWorks
VARIABLES p, phase, os, pending, delayReady, ageReady, signal, due, seen, evidence
vars == <<p, phase, os, pending, delayReady, ageReady, signal, due, seen, evidence>>
c == [strategy |-> Strategy, attempts |-> Budget, max_failed |-> MaxFailed]

Init ==
    /\ p = EmptyProcess /\ phase = "Running" /\ os = "None" /\ pending = FALSE
    /\ delayReady = FALSE /\ ageReady = FALSE /\ signal = "None" /\ due = FALSE
    /\ seen = FALSE /\ evidence = FALSE

Admit ==
    /\ p.status = "Initial" /\ phase = "Running"
    /\ p' = [p EXCEPT !.status = "Starting", !.hc = -1]
    /\ pending' = TRUE /\ delayReady' = FALSE /\ ageReady' = FALSE /\ evidence' = FALSE
    /\ UNCHANGED <<phase, os, signal, due, seen>>
Delay ==
    /\ pending /\ ~delayReady /\ delayReady' = TRUE
    /\ UNCHANGED <<p, phase, os, pending, ageReady, signal, due, seen, evidence>>
Spawn ==
    /\ pending /\ delayReady /\ phase = "Running"
    /\ pending' = FALSE /\ os' = "Alive"
    /\ UNCHANGED <<p, phase, delayReady, ageReady, signal, due, seen, evidence>>
SpawnFail ==
    /\ pending /\ (phase # "Running" \/ Scenario = "Failures")
    /\ pending' = FALSE
    /\ p' = [p EXCEPT !.status = "Failed", !.retries = @ + 1]
    /\ UNCHANGED <<phase, os, delayReady, ageReady, signal, due, seen, evidence>>
Register ==
    /\ p.status = "Starting" /\ ~pending /\ os \in {"Alive", "Dead"}
    /\ p' = [p EXCEPT !.status = "Started", !.pid = 1]
    /\ UNCHANGED <<phase, os, pending, delayReady, ageReady, signal, due, seen, evidence>>
Age ==
    /\ os = "Alive" /\ ~ageReady /\ ageReady' = TRUE
    /\ UNCHANGED <<p, phase, os, pending, delayReady, signal, due, seen, evidence>>
Health ==
    /\ p.status \in {"Started", "Running"}
    /\ \E good \in BOOLEAN:
         /\ (good => ageReady)
         /\ (~good => Scenario = "Failures")
         /\ (~good => p.hc <= MaxFailed)
         /\ p' = HealthProcess(p, good)
         /\ evidence' = (evidence \/ good)
    /\ UNCHANGED <<phase, os, pending, delayReady, ageReady, signal, due, seen>>
Ready ==
    /\ p.status = "Started" /\ p.hc = 0 /\ phase = "Running"
    /\ p' = Update(p, "Running", c) /\ seen' = TRUE
    /\ UNCHANGED <<phase, os, pending, delayReady, ageReady, signal, due, evidence>>
Shutdown ==
    /\ phase = "Running" /\ (Scenario # "Startup" \/ seen)
    /\ phase' = "Graceful"
    /\ UNCHANGED <<p, os, pending, delayReady, ageReady, signal, due, seen, evidence>>
Kill ==
    /\ p.status \in {"Started", "Running"}
    /\ (phase # "Running" \/ Exceeded(p, c))
    /\ p' = [Update(p, "InKilling", c) EXCEPT !.timer = TRUE]
    /\ signal' = "Term" /\ due' = FALSE
    /\ UNCHANGED <<phase, os, pending, delayReady, ageReady, seen, evidence>>
Time ==
    /\ p.status = "InKilling" /\ ~due /\ due' = TRUE
    /\ UNCHANGED <<p, phase, os, pending, delayReady, ageReady, signal, seen, evidence>>
Force ==
    /\ p.status = "InKilling" /\ (due \/ phase = "Forceful") /\ signal # "Kill"
    /\ signal' = "Kill"
    /\ UNCHANGED <<p, phase, os, pending, delayReady, ageReady, due, seen, evidence>>
Escalate ==
    /\ phase = "Graceful" /\ phase' = "Forceful"
    /\ UNCHANGED <<p, os, pending, delayReady, ageReady, signal, due, seen, evidence>>
Exit ==
    /\ os = "Alive" /\ Scenario = "Failures" /\ os' = "Dead"
    /\ UNCHANGED <<p, phase, pending, delayReady, ageReady, signal, due, seen, evidence>>
SignalExit ==
    /\ os = "Alive" /\ (signal = "Kill" \/ (TermWorks /\ signal = "Term"))
    /\ os' = "Dead"
    /\ UNCHANGED <<p, phase, pending, delayReady, ageReady, signal, due, seen, evidence>>
Reap ==
    /\ os = "Dead" /\ p.pid = 1
    /\ \E success \in BOOLEAN: p' = ExitProcess(p, c, success)
    /\ os' = "None" /\ signal' = "None" /\ due' = FALSE /\ evidence' = FALSE
    /\ UNCHANGED <<phase, pending, delayReady, ageReady, seen>>
Decide ==
    /\ p.status \in {"Success", "Failed"}
    /\ p' = Update(p, IF phase = "Running" THEN RestartTarget(p, c)
                     ELSE IF p.status = "Failed" THEN "FinishedFailed" ELSE "Finished", c)
    /\ UNCHANGED <<phase, os, pending, delayReady, ageReady, signal, due, seen, evidence>>
StopInitial ==
    /\ p.status = "Initial" /\ phase # "Running"
    /\ p' = Update(p, "Finished", c)
    /\ UNCHANGED <<phase, os, pending, delayReady, ageReady, signal, due, seen, evidence>>
Done == p.status \in Terminal /\ UNCHANGED vars
Next == Admit \/ Delay \/ Spawn \/ SpawnFail \/ Register \/ Age \/ Health
        \/ Ready \/ Shutdown \/ Kill \/ Time \/ Force \/ Escalate
        \/ Exit \/ SignalExit \/ Reap \/ Decide \/ StopInitial \/ Done
Spec == Init /\ [][Next]_vars
    /\ WF_vars(Admit) /\ WF_vars(Delay) /\ WF_vars(Spawn) /\ WF_vars(SpawnFail)
    /\ WF_vars(Register) /\ WF_vars(Age) /\ WF_vars(Health) /\ WF_vars(Ready)
    /\ WF_vars(Shutdown) /\ WF_vars(Kill) /\ WF_vars(Time) /\ WF_vars(Force)
    /\ WF_vars(SignalExit) /\ WF_vars(Reap) /\ WF_vars(Decide) /\ WF_vars(StopInitial)
TypeOK == WellTyped(p) /\ phase \in {"Running", "Graceful", "Forceful"}
          /\ os \in {"None", "Alive", "Dead"} /\ signal \in {"None", "Term", "Kill"}
          /\ pending \in BOOLEAN /\ delayReady \in BOOLEAN /\ ageReady \in BOOLEAN
          /\ due \in BOOLEAN /\ seen \in BOOLEAN /\ evidence \in BOOLEAN
Clean == p.status \in Terminal => (p.pid = 0 /\ os = "None" /\ ~pending)
Ownership == p.pid # 0 => os # "None"
Readiness == p.status = "Running" => (evidence /\ ageReady /\ p.pid # 0)
Coherence == (pending \/ os # "None") => p.status \in Live \cup {"InKilling"}
LegalSteps == [][p'.status = p.status \/ p.status \in Allowed(p'.status)]_vars
ShutdownProgress == phase # "Running" ~> p.status \in Terminal
StartupProgress == <>seen
FailurePreserved ==
    [][(phase # "Running" /\ p.status = "Failed") =>
       p'.status \in {"Failed", "FinishedFailed"}]_vars
=============================================================================
