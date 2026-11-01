---------------------------- MODULE Supervisor ----------------------------
EXTENDS FiniteSets
CONSTANTS Services, StartDeps(_), StopDeps(_), KillTargets(_), FailurePolicy,
          ExternalShutdown
VARIABLES phase, st, owned, active, blocked
vars == <<phase, st, owned, active, blocked>>
Live == {"Preparing", "Up", "Stopping"}
Exited == {"ExitOk", "ExitFail"}
Terminal == {"Done", "DoneFail"}
StartReady(s) == \A d \in StartDeps(s): st[d] \in {"Up", "Done"}
StopReady(s) == \A d \in StopDeps(s): st[d] \in Terminal
StopAllowed(s) == s \in blocked \/ phase = "Forceful"
                  \/ (phase = "Graceful" /\ StopReady(s))
Init ==
    /\ phase = "Running" /\ st = [s \in Services |-> "Waiting"]
    /\ owned = {} /\ active = {} /\ blocked = {}
Admit(s) ==
    /\ phase = "Running" /\ st[s] = "Waiting" /\ s \notin blocked /\ StartReady(s)
    /\ st' = [st EXCEPT ![s] = "Preparing"] /\ active' = active \cup {s}
    /\ UNCHANGED <<phase, owned, blocked>>
Spawn(s) ==
    /\ s \in active /\ phase = "Running" /\ s \notin blocked
    /\ active' = active \ {s} /\ owned' = owned \cup {s}
    /\ UNCHANGED <<phase, st, blocked>>
Abort(s) ==
    /\ s \in active
    /\ active' = active \ {s} /\ st' = [st EXCEPT ![s] = "ExitFail"]
    /\ UNCHANGED <<phase, owned, blocked>>
Ready(s) ==
    /\ st[s] = "Preparing" /\ s \in owned /\ phase = "Running" /\ s \notin blocked
    /\ st' = [st EXCEPT ![s] = "Up"]
    /\ UNCHANGED <<phase, owned, active, blocked>>
Stop(s) ==
    /\ st[s] \in {"Preparing", "Up"} /\ s \in owned /\ StopAllowed(s)
    /\ st' = [st EXCEPT ![s] = "Stopping"]
    /\ UNCHANGED <<phase, owned, active, blocked>>
Exit(s) ==
    /\ s \in owned /\ st[s] \in Live
    /\ \E result \in Exited: st' = [st EXCEPT ![s] = result]
    /\ owned' = owned \ {s}
    /\ UNCHANGED <<phase, active, blocked>>
Retire(s) ==
    /\ st[s] \in Exited
    /\ st' = [st EXCEPT ![s] = IF st[s] = "ExitFail" THEN "DoneFail" ELSE "Done"]
    /\ phase' = IF st[s] = "ExitFail" /\ phase = "Running" /\ FailurePolicy = "Shutdown"
                THEN "Graceful" ELSE phase
    /\ blocked' = IF st[s] = "ExitFail" /\ phase = "Running"
                  THEN blocked \cup KillTargets(s) ELSE blocked
    /\ UNCHANGED <<owned, active>>
Cancel(s) ==
    /\ st[s] = "Waiting" /\ (phase # "Running" \/ s \in blocked)
    /\ \E result \in (IF s \in blocked THEN {"ExitOk", "Done"} ELSE {"Done"}):
         st' = [st EXCEPT ![s] = result]
    /\ UNCHANGED <<phase, owned, active, blocked>>
RequestStop ==
    /\ ExternalShutdown /\ phase = "Running" /\ phase' = "Graceful"
    /\ UNCHANGED <<st, owned, active, blocked>>
Escalate ==
    /\ phase = "Graceful" /\ phase' = "Forceful"
    /\ UNCHANGED <<st, owned, active, blocked>>
AllDone == \A s \in Services: st[s] \in Terminal
Done == AllDone /\ UNCHANGED vars
Next == RequestStop \/ Escalate \/ Done
        \/ \E s \in Services: Admit(s) \/ Spawn(s) \/ Abort(s) \/ Ready(s)
                             \/ Stop(s) \/ Exit(s) \/ Retire(s) \/ Cancel(s)
StoppedExit(s) == st[s] = "Stopping" /\ Exit(s)
RequiredAbort(s) == (phase # "Running" \/ s \in blocked) /\ Abort(s)
Spec == Init /\ [][Next]_vars
        /\ \A s \in Services:
             WF_vars(Admit(s)) /\ WF_vars(Spawn(s)) /\ WF_vars(Ready(s))
             /\ WF_vars(RequiredAbort(s)) /\ WF_vars(Stop(s))
             /\ WF_vars(StoppedExit(s)) /\ WF_vars(Retire(s)) /\ WF_vars(Cancel(s))
TypeOK ==
    /\ phase \in {"Running", "Graceful", "Forceful"}
    /\ st \in [Services -> {"Waiting"} \cup Live \cup Exited \cup Terminal]
    /\ owned \subseteq Services /\ active \subseteq Services /\ blocked \subseteq Services
Ownership ==
    /\ owned \cap active = {}
    /\ \A s \in owned: st[s] \in Live
    /\ \A s \in active: st[s] = "Preparing"
ShutdownProgress == phase # "Running" ~> AllDone
VictimProgress == \A s \in Services: s \in blocked ~> st[s] \in Terminal
=============================================================================
