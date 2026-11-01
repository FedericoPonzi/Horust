------------------------------ MODULE Horust ------------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
CONSTANTS N, Graph, StopGraph, Scenario, QueueBound, FailurePolicy, DieIfFailed,
          ExternalShutdown
Services == 1..N
Terminal == {"Finished", "FinishedFailed"}
Deps(s) ==
    CASE Graph = "Chain" -> IF s = 1 THEN {} ELSE {s - 1}
      [] Graph = "FanOut" -> IF s = 1 THEN {} ELSE {1}
      [] Graph = "FanIn" -> IF s = N THEN Services \ {N} ELSE {}
      [] Graph = "Cycle" -> {IF s = 1 THEN N ELSE s - 1}
      [] OTHER -> {}
StopDeps(s) ==
    CASE StopGraph = "ReverseStartup" -> {t \in Services : s \in Deps(t)}
      [] StopGraph = "Chain" -> IF s = N THEN {} ELSE {s + 1}
      [] StopGraph = "Cycle" -> {IF s = N THEN 1 ELSE s + 1}
      [] OTHER -> {}
VARIABLES state, os, pending, signalled, phase, q, seen, failed, victims
vars == <<state, os, pending, signalled, phase, q, seen, failed, victims>>
ReadyDeps(s) == \A d \in Deps(s): state[d] \in {"Running", "Finished"}
StoppedDeps(s) == \A d \in StopDeps(s): state[d] \in Terminal
Finished == \A s \in Services: state[s] \in Terminal
Init ==
    /\ state = [s \in Services |-> "Initial"]
    /\ os = [s \in Services |-> "None"]
    /\ pending = {} /\ signalled = {} /\ phase = "Running" /\ q = <<>>
    /\ seen = {} /\ failed = {} /\ victims = {}
Command(s) ==
    CASE state[s] = "Initial" /\ s \in victims -> "StopVictim"
      [] state[s] = "Initial" ->
             IF phase = "Running" /\ ReadyDeps(s) THEN "Run"
             ELSE IF phase # "Running" THEN "Finish" ELSE "None"
      [] state[s] = "Started" ->
             IF s \in victims THEN "Kill"
             ELSE IF phase = "Running" THEN "Ready"
             ELSE IF phase = "Forceful" \/ StoppedDeps(s) THEN "Kill" ELSE "None"
      [] state[s] = "Running" ->
             IF (phase # "Running" /\ (phase = "Forceful" \/ StoppedDeps(s)))
                 \/ s \in victims THEN "Kill" ELSE "None"
      [] state[s] \in {"Success", "Failed"} -> "Finalize"
      [] OTHER -> "None"
Generate(s) ==
    /\ Command(s) # "None" /\ Len(q) < QueueBound
    /\ q' = Append(q, [kind |-> Command(s), svc |-> s])
    /\ UNCHANGED <<state, os, pending, signalled, phase, seen, failed, victims>>
Spawn(s) ==
    /\ s \in pending /\ Len(q) < QueueBound
    /\ IF phase = "Running" /\ s \notin victims
       THEN /\ os' = [os EXCEPT ![s] = "Alive"]
            /\ q' = Append(q, [kind |-> "Pid", svc |-> s])
       ELSE /\ UNCHANGED os
            /\ q' = Append(q, [kind |-> "SpawnFailed", svc |-> s])
    /\ pending' = pending \ {s}
    /\ UNCHANGED <<state, signalled, phase, seen, failed, victims>>
SpontaneousExit(s) ==
    /\ Scenario = "Failures" /\ os[s] = "Alive"
    /\ \E outcome \in {"DeadSuccess", "DeadFailure"}: os' = [os EXCEPT ![s] = outcome]
    /\ UNCHANGED <<state, pending, signalled, phase, q, seen, failed, victims>>
TimeoutExit(s) ==
    /\ s \in signalled /\ os[s] = "Alive"
    /\ os' = [os EXCEPT ![s] = "DeadFailure"]
    /\ UNCHANGED <<state, pending, signalled, phase, q, seen, failed, victims>>
Reap(s) ==
    /\ os[s] \in {"DeadSuccess", "DeadFailure"} /\ state[s] \in {"Started", "Running", "InKilling"}
    /\ Len(q) < QueueBound
    /\ os' = [os EXCEPT ![s] = "Reaped"]
    /\ q' = Append(q, [kind |-> IF os[s] = "DeadSuccess" THEN "ExitSuccess" ELSE "ExitFailure", svc |-> s])
    /\ UNCHANGED <<state, pending, signalled, phase, seen, failed, victims>>
Deliver ==
    /\ Len(q) > 0
    /\ LET e == Head(q)
           s == e.svc
           canRun == e.kind = "Run" /\ state[s] = "Initial" /\ phase = "Running"
                     /\ ReadyDeps(s) /\ s \notin victims
           canKill == e.kind = "Kill" /\ state[s] \in {"Started", "Running"}
                      /\ (s \in victims \/ phase = "Forceful"
                          \/ (phase = "Graceful" /\ StoppedDeps(s)))
           isExit == e.kind \in {"ExitSuccess", "ExitFailure"}
           failure == e.kind = "Finalize" /\ state[s] = "Failed" /\ s \notin failed
                      /\ phase = "Running"
           target == CASE canRun -> "Starting"
                       [] e.kind = "Pid" /\ state[s] = "Starting" -> "Started"
                       [] e.kind = "SpawnFailed" /\ state[s] = "Starting" -> "Failed"
                       [] e.kind = "Ready" /\ state[s] = "Started" /\ phase = "Running"
                          /\ s \notin victims /\ os[s] \in {"Alive", "DeadSuccess", "DeadFailure"} -> "Running"
                       [] canKill -> "InKilling"
                       [] e.kind = "ExitSuccess" -> "Success"
                       [] e.kind = "ExitFailure" -> "Failed"
                       [] e.kind = "StopVictim" /\ state[s] = "Initial" /\ s \in victims -> "Success"
                       [] e.kind = "Finish" /\ state[s] = "Initial" /\ phase # "Running" -> "Finished"
                       [] e.kind = "Finalize" /\ state[s] = "Success" -> "Finished"
                       [] e.kind = "Finalize" /\ state[s] = "Failed" -> "FinishedFailed"
                       [] OTHER -> state[s]
       IN /\ state' = [state EXCEPT ![s] = target]
          /\ pending' = IF canRun THEN pending \cup {s} ELSE pending
          /\ signalled' = IF canKill THEN signalled \cup {s}
                          ELSE IF isExit THEN signalled \ {s} ELSE signalled
          /\ os' = IF isExit THEN [os EXCEPT ![s] = "None"] ELSE os
          /\ seen' = IF target = "Running" THEN seen \cup {s} ELSE seen
          /\ phase' = IF failure /\ FailurePolicy = "Shutdown" /\ phase = "Running"
                       THEN "Graceful" ELSE phase
          /\ failed' = IF failure THEN failed \cup {s} ELSE failed
          /\ victims' = IF failure /\ (FailurePolicy = "KillDependents" \/ DieIfFailed)
                         THEN victims \cup {t \in Services: s \in Deps(t)} ELSE victims
    /\ q' = Tail(q)
Shutdown ==
    /\ ExternalShutdown /\ phase = "Running" /\ (Scenario # "Startup" \/ seen = Services)
    /\ phase' = "Graceful"
    /\ UNCHANGED <<state, os, pending, signalled, q, seen, failed, victims>>
Escalate ==
    /\ phase = "Graceful" /\ phase' = "Forceful"
    /\ UNCHANGED <<state, os, pending, signalled, q, seen, failed, victims>>
Done == Finished /\ Len(q) = 0 /\ UNCHANGED vars
Next == (\E s \in Services: Generate(s) \/ Spawn(s) \/ SpontaneousExit(s)
                           \/ TimeoutExit(s) \/ Reap(s))
        \/ Deliver \/ Shutdown \/ Escalate \/ Done
Spec == Init /\ [][Next]_vars /\ WF_vars(Deliver) /\ WF_vars(Shutdown)
        /\ (\A s \in Services:
             SF_vars(Generate(s)) /\ SF_vars(Spawn(s)) /\ WF_vars(TimeoutExit(s))
             /\ SF_vars(Reap(s)))
TypeOK ==
    /\ state \in [Services -> {"Initial", "Starting", "Started", "Running",
                               "InKilling", "Success", "Failed"} \cup Terminal]
    /\ os \in [Services -> {"None", "Alive", "DeadSuccess", "DeadFailure", "Reaped"}]
    /\ phase \in {"Running", "Graceful", "Forceful"}
    /\ pending \subseteq Services /\ signalled \subseteq Services
    /\ seen \subseteq Services /\ failed \subseteq Services /\ victims \subseteq Services
    /\ Len(q) <= QueueBound
Clean == \A s \in Services: state[s] \in Terminal => os[s] = "None" /\ s \notin pending
DependencySafety ==
    [][\A s \in Services:
        (state[s] = "Initial" /\ state'[s] = "Starting") =>
        (phase = "Running" /\ ReadyDeps(s) /\ s \notin victims)]_vars
NoLateSpawn ==
    [][\A s \in Services:
        (os[s] # "Alive" /\ os'[s] = "Alive") =>
        (phase = "Running" /\ s \notin victims)]_vars
ReadySafety ==
    [][\A s \in Services:
        (state[s] = "Started" /\ state'[s] = "Running") =>
        (phase = "Running" /\ s \notin victims
         /\ os[s] \in {"Alive", "DeadSuccess", "DeadFailure"})]_vars
ShutdownOrder ==
    [][\A s \in Services:
        (state[s] # "InKilling" /\ state'[s] = "InKilling") =>
        (phase = "Forceful" \/ StoppedDeps(s) \/ s \in victims)]_vars
Monotone == [][phase # "Running" => phase' # "Running"]_vars
StartupProgress == <>(seen = Services)
ShutdownProgress == phase # "Running" ~> Finished
FailureShutdown == \A s \in Services: (state[s] = "Failed" /\ phase = "Running") ~> phase # "Running"
VictimProgress == \A s \in Services: s \in victims ~> state[s] \in Terminal
IgnoreStable == [] (phase = "Running" /\ victims = {})
KillTargets(s) ==
    IF FailurePolicy = "KillDependents" \/ DieIfFailed
    THEN {t \in Services: s \in Deps(t)} ELSE {}
AbstractState(s) ==
    CASE state[s] = "Initial" -> "Waiting"
      [] state[s] \in {"Starting", "Started"} -> "Preparing"
      [] state[s] = "Running" -> "Up"
      [] state[s] = "InKilling" -> "Stopping"
      [] state[s] = "Success" -> "ExitOk"
      [] state[s] = "Failed" -> "ExitFail"
      [] state[s] = "Finished" -> "Done"
      [] state[s] = "FinishedFailed" -> "DoneFail"
Abstract == INSTANCE Supervisor WITH
    StartDeps <- Deps, StopDeps <- StopDeps,
    st <- [s \in Services |-> AbstractState(s)],
    owned <- {s \in Services: os[s] # "None"},
    active <- {s \in Services: state[s] = "Starting" /\ os[s] = "None"},
    blocked <- victims
AbstractSpec == Abstract!Spec
=============================================================================
