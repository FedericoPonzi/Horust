----------------------------- MODULE Process -----------------------------
EXTENDS Naturals, TLC

CONSTANTS Scenario, TermWorks
VARIABLES state, os, registered, pending, healthy, phase, everRunning, signal, deadline
vars == <<state, os, registered, pending, healthy, phase, everRunning, signal, deadline>>
Terminal == {"Finished", "FinishedFailed"}
States == {"Initial", "Starting", "Started", "Running", "InKilling",
           "Success", "Failed"} \cup Terminal

Init ==
    /\ state = "Initial"
    /\ os = "None"
    /\ registered = FALSE
    /\ pending = FALSE
    /\ healthy = FALSE
    /\ phase = "Running"
    /\ everRunning = FALSE
    /\ signal = "None" /\ deadline = FALSE

Admit ==
    /\ state = "Initial" /\ phase = "Running"
    /\ state' = "Starting" /\ pending' = TRUE
    /\ UNCHANGED <<os, registered, healthy, phase, everRunning, signal, deadline>>

Spawn ==
    /\ pending /\ phase = "Running"
    /\ pending' = FALSE /\ os' = "Alive"
    /\ UNCHANGED <<state, registered, healthy, phase, everRunning, signal, deadline>>

SpawnFailed ==
    /\ pending /\ (Scenario # "Startup" \/ phase # "Running")
    /\ pending' = FALSE /\ state' = "Failed"
    /\ UNCHANGED <<os, registered, healthy, phase, everRunning, signal, deadline>>

Register ==
    /\ state = "Starting" /\ os \in {"Alive", "Dead"} /\ ~registered
    /\ registered' = TRUE /\ state' = "Started"
    /\ UNCHANGED <<os, pending, healthy, phase, everRunning, signal, deadline>>

Ready ==
    /\ state = "Started" /\ os = "Alive" /\ phase = "Running"
    /\ state' = "Running" /\ healthy' = TRUE /\ everRunning' = TRUE
    /\ UNCHANGED <<os, registered, pending, phase, signal, deadline>>

SpontaneousExit ==
    /\ os = "Alive"
    /\ (Scenario # "Startup" \/ everRunning \/ phase # "Running")
    /\ os' = "Dead"
    /\ UNCHANGED <<state, registered, pending, healthy, phase, everRunning, signal, deadline>>

SignalledExit ==
    /\ os = "Alive" /\ (signal = "Kill" \/ (TermWorks /\ signal = "Term"))
    /\ os' = "Dead"
    /\ UNCHANGED <<state, registered, pending, healthy, phase, everRunning, signal, deadline>>

Reap ==
    /\ os = "Dead" /\ registered
    /\ os' = "None" /\ registered' = FALSE
    /\ state' \in {"Success", "Failed"}
    /\ healthy' = FALSE /\ signal' = "None" /\ deadline' = FALSE
    /\ UNCHANGED <<pending, phase, everRunning>>

Finalize ==
    /\ state \in {"Success", "Failed"} \/ (state = "Initial" /\ phase # "Running")
    /\ state' = IF state = "Failed" THEN "FinishedFailed" ELSE "Finished"
    /\ UNCHANGED <<os, registered, pending, healthy, phase, everRunning, signal, deadline>>

Shutdown ==
    /\ phase = "Running"
    /\ (Scenario # "Startup" \/ everRunning)
    /\ phase' = "Graceful"
    /\ UNCHANGED <<state, os, registered, pending, healthy, everRunning, signal, deadline>>

Escalate ==
    /\ phase = "Graceful" /\ deadline /\ phase' = "Forceful"
    /\ UNCHANGED <<state, os, registered, pending, healthy, everRunning, signal, deadline>>

Kill ==
    /\ phase # "Running" /\ state \in {"Started", "Running"}
    /\ state' = "InKilling" /\ signal' = "Term" /\ deadline' = FALSE
    /\ UNCHANGED <<os, registered, pending, healthy, phase, everRunning>>

Time ==
    /\ state = "InKilling" /\ ~deadline /\ deadline' = TRUE
    /\ UNCHANGED <<state, os, registered, pending, healthy, phase, everRunning, signal>>

ForceKill ==
    /\ state = "InKilling" /\ (deadline \/ phase = "Forceful")
    /\ signal # "Kill" /\ signal' = "Kill"
    /\ UNCHANGED <<state, os, registered, pending, healthy, phase, everRunning, deadline>>

Done == state \in Terminal /\ UNCHANGED vars
Next == Admit \/ Spawn \/ SpawnFailed \/ Register \/ Ready \/ SpontaneousExit
        \/ SignalledExit \/ Reap \/ Finalize \/ Shutdown \/ Escalate \/ Kill
        \/ Time \/ ForceKill \/ Done
Spec == Init /\ [][Next]_vars
        /\ WF_vars(Admit) /\ WF_vars(Spawn) /\ WF_vars(SpawnFailed)
        /\ WF_vars(Register) /\ WF_vars(Ready) /\ WF_vars(SignalledExit)
        /\ WF_vars(Reap) /\ WF_vars(Finalize) /\ WF_vars(Shutdown)
        /\ WF_vars(Kill) /\ WF_vars(Time) /\ WF_vars(ForceKill) /\ WF_vars(Escalate)

TypeOK ==
    /\ state \in States /\ os \in {"None", "Alive", "Dead"}
    /\ registered \in BOOLEAN /\ pending \in BOOLEAN /\ healthy \in BOOLEAN
    /\ everRunning \in BOOLEAN /\ phase \in {"Running", "Graceful", "Forceful"}
    /\ signal \in {"None", "Term", "Kill"} /\ deadline \in BOOLEAN
Accounting == registered => os # "None"
TerminalClean == state \in Terminal => (os = "None" /\ ~pending /\ ~registered)
Readiness == state = "Running" => (healthy /\ registered)
Coherence ==
    /\ (pending => os = "None" /\ state = "Starting")
    /\ (state \in {"Initial", "Success", "Failed"} \cup Terminal => os = "None")
    /\ (registered => state \in {"Started", "Running", "InKilling"})
    /\ (healthy => os # "None")
StartupProgress == <>everRunning
ShutdownProgress == phase # "Running" ~> state \in Terminal
ShutdownMonotone == [][phase' = "Running" => phase = "Running"]_vars
=============================================================================
