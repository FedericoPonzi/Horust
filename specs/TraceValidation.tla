-------------------------- MODULE TraceValidation --------------------------
EXTENDS TraceData, ProcessRules, TLC, FiniteSets
VARIABLES index, snapshot, valid
vars == <<index, snapshot, valid>>
Names == DOMAIN Config
Values(seq) == {seq[i]: i \in 1..Len(seq)}
Event(kind, svc, target) ==
    [kind |-> kind, svc |-> svc, target |-> target, pid |-> 0,
     code |-> 0, good |-> FALSE, mode |-> "", attempt_id |-> 0]
Status(svc, target) == Event("StatusUpdate", svc, target)
Changed(svc, target) == Event("StatusChanged", svc, target)
KillEvents(svc) == <<Status(svc, "InKilling"), Event("Kill", svc, "")>>
ShutdownEvent == [Event("Shutdown", "", "") EXCEPT !.mode = "Graceful"]
MaxAttempt == AttemptMaximum
TrackedEmpty == [attempt_id |-> 0] @@ EmptyProcess
RECURSIVE Flatten(_)
Flatten(seqs) == IF Len(seqs) = 0 THEN <<>> ELSE Head(seqs) \o Flatten(Tail(seqs))
ReadyDeps(s, n) == \A d \in Values(Config[n].start_after):
                    s.services[d].status \in {"Running", "Finished"}
StoppedDeps(s, n) == \A d \in Values(Config[n].shutdown_after):
                      s.services[d].status \in Terminal
Change(s, n, p) == [s EXCEPT !.services[n] = p]
PidPairs(s) == {<<s.services[n].pid, n>>: n \in {t \in Names: s.services[t].pid # 0}}
SnapshotOK(s) ==
    /\ DOMAIN s.services = Names
    /\ s.phase \in {"Running", "Graceful", "Forceful"}
    /\ \A n \in Names:
         /\ WellTyped(s.services[n])
         /\ s.services[n].attempt_id \in 0..MaxAttempt
         /\ (s.services[n].status \in Terminal \cup {"Initial", "Success", "Failed"}
              => s.services[n].pid = 0)
    /\ s.pid_map = PidPairs(s)
    /\ \A a, b \in Names:
         (s.services[a].pid # 0 /\ s.services[a].pid = s.services[b].pid) => a = b
InitialOK == Initial.phase = "Running" /\ Initial.pid_map = {}
             /\ DOMAIN Initial.services = Names
             /\ \A n \in Names: Initial.services[n] = TrackedEmpty

Result(s, e) ==
    IF e.kind = "Shutdown"
    THEN [s EXCEPT !.phase = IF s.phase = "Forceful" THEN "Forceful" ELSE e.mode]
    ELSE LET p == s.services[e.svc]
             c == Config[e.svc]
         IN CASE e.kind = "Run" /\ p.status = "Initial" ->
                      IF p.attempt_id = MaxAttempt
                      THEN [Change(s, e.svc, [p EXCEPT !.status = "FinishedFailed"])
                            EXCEPT !.phase = IF @ = "Forceful" THEN @ ELSE "Graceful"]
                      ELSE IF s.phase # "Running" THEN s
                      ELSE Change(s, e.svc, [p EXCEPT !.status = "Starting", !.hc = -1,
                                            !.attempt_id = @ + 1])
              [] e.kind = "SpawnFailed" ->
                      Change(s, e.svc, [p EXCEPT !.status = "Failed", !.retries = @ + 1])
              [] e.kind = "SpawnRegistered" ->
                      [Change(s, e.svc, [p EXCEPT !.pid = e.pid,
                         !.status = "Started"])
                       EXCEPT !.pid_map = @ \cup {<<e.pid, e.svc>>}]
              [] e.kind = "HealthCheck" ->
                      IF e.attempt_id = p.attempt_id THEN Change(s, e.svc, HealthProcess(p, e.good))
                      ELSE s
              [] e.kind = "StatusUpdate" ->
                      IF (e.target = "Running" /\ p.status = "Started" /\ HasFailed(p))
                         \/ (e.target = "Initial" /\ p.status \in {"Success", "Failed"}
                             /\ s.phase # "Running")
                      THEN s ELSE Change(s, e.svc, Update(p, e.target, c))
              [] e.kind = "ReapedExit" ->
                      [Change(s, e.svc, ExitProcess(p, c, e.code \in Values(c.successful_codes)))
                       EXCEPT !.pid_map = {pair \in @: pair[1] # p.pid}]
              [] e.kind = "Kill" /\ p.status = "InKilling" ->
                      Change(s, e.svc, [p EXCEPT !.timer = TRUE])
              [] OTHER -> s

Guards(s, e) ==
    IF e.kind = "Shutdown" THEN e.mode \in {"Graceful", "Forceful"}
    ELSE IF e.svc \notin Names THEN FALSE
    ELSE LET p == s.services[e.svc]
             c == Config[e.svc]
         IN /\ (e.kind = "Run" /\ p.status = "Initial" =>
                  p.pid = 0 /\ (p.attempt_id = MaxAttempt
                      \/ s.phase # "Running" \/ ReadyDeps(s, e.svc)))
            /\ (e.kind = "SpawnRegistered" =>
                  s.phase = "Running" /\ p.status = "Starting" /\ p.pid = 0 /\ e.pid > 0
                  /\ \A n \in Names: s.services[n].pid # e.pid)
            /\ (e.kind = "SpawnFailed" => p.status \in {"Starting", "InKilling"} /\ p.pid = 0)
            /\ (e.kind = "ReapedExit" => p.pid = e.pid /\ e.pid > 0
                  /\ p.status \in Live \cup {"InKilling"})
            /\ (e.kind = "StatusUpdate" /\ e.target = "Initial" /\ p.status \in {"Failed", "Success"} =>
                  p.pid = 0 /\ (s.phase # "Running" \/ RestartTarget(p, c) = "Initial"))
            /\ (e.kind = "StatusUpdate" /\ e.target = "InKilling" /\ s.phase = "Graceful"
                 /\ p.status \in {"Started", "Running"} => StoppedDeps(s, e.svc))

Outputs(s, e, after) ==
    IF e.kind = "Shutdown" THEN <<>>
    ELSE LET p == s.services[e.svc]
         IN CASE e.kind = "Run" /\ p.status = "Initial" ->
                   IF p.attempt_id = MaxAttempt
                   THEN <<Changed(e.svc, "FinishedFailed"),
                          [ShutdownEvent EXCEPT !.mode = after.phase]>>
                   ELSE IF s.phase # "Running" THEN <<>>
                   ELSE <<Changed(e.svc, "Starting")>>
              [] e.kind = "SpawnFailed" -> <<Status(e.svc, "Failed")>>
              [] e.kind = "SpawnRegistered" ->
                   <<[Event("PidChanged", e.svc, "") EXCEPT !.pid = e.pid], Changed(e.svc, "Started"),
                     [Event("StartHealthCheck", e.svc, "") EXCEPT !.attempt_id = p.attempt_id]>>
              [] e.kind = "ReapedExit" -> <<Changed(e.svc, after.services[e.svc].status)>>
              [] e.kind = "StatusUpdate" /\ p.status # after.services[e.svc].status ->
                   <<Changed(e.svc, after.services[e.svc].status)>>
              [] OTHER -> <<>>

PrepareFailure(s, a) ==
    LET e == a.input
        p == s.services[e.svc]
    IN /\ e.kind = "Run" /\ p.status = "Initial" /\ s.phase = "Running"
       /\ p.attempt_id < MaxAttempt
       /\ a.after = [Change(s, e.svc, [p EXCEPT !.status = "FinishedFailed", !.hc = -1,
                                    !.attempt_id = @ + 1])
                     EXCEPT !.phase = "Graceful"]
       /\ a.outputs = <<Status(e.svc, "FinishedFailed"), ShutdownEvent>>

StepOK(s, a) ==
    /\ a.before = s
    /\ Guards(s, a.input)
    /\ (a.signal_count > 0 =>
         a.input.mode = IF s.phase = "Running" /\ a.signal_count = 1 THEN "Graceful" ELSE "Forceful")
    /\ IF a.input.kind # "Shutdown" /\ a.input.svc \notin Names THEN FALSE
       ELSE (a.after = Result(s, a.input) /\ a.outputs = Outputs(s, a.input, a.after))
            \/ (a.input.kind = "Run" /\ PrepareFailure(s, a))

Failures(s, n, order) ==
    LET dependents == SelectSeq(order, LAMBDA t: n \in Values(Config[t].start_after))
        dying == SelectSeq(order, LAMBDA t: n \in Values(Config[t].die_if_failed))
        policy == Config[n].failure
    IN (CASE policy = "Shutdown" -> <<ShutdownEvent>>
          [] policy = "KillDependents" -> Flatten([i \in 1..Len(dependents) |-> KillEvents(dependents[i])])
          [] OTHER -> <<>>)
       \o <<Status(n, RestartTarget(s.services[n], Config[n]))>>
       \o Flatten([i \in 1..Len(dying) |-> KillEvents(dying[i])])

NextEvents(s, n, due, order) ==
    LET p == s.services[n]
        c == Config[n]
    IN IF s.phase = "Running"
       THEN CASE p.status = "Initial" /\ ReadyDeps(s, n) -> <<Event("Run", n, "")>>
              [] p.status = "Started" /\ ~HasFailed(p) -> <<Status(n, "Running")>>
              [] p.status \in {"Running", "Started"} /\ Exceeded(p, c) -> KillEvents(n)
              [] p.status = "Success" -> <<Status(n, RestartTarget(p, c))>>
              [] p.status = "Failed" -> Failures(s, n, order)
              [] p.status = "InKilling" /\ due -> <<Event("ForceKill", n, "")>>
              [] OTHER -> <<>>
       ELSE CASE p.status \in {"Running", "Started"} /\ (s.phase = "Forceful" \/ StoppedDeps(s, n)) ->
                     KillEvents(n)
              [] p.status \in {"Success", "Initial"} -> <<Status(n, "Finished")>>
              [] p.status = "Failed" -> <<Status(n, "FinishedFailed")>>
              [] p.status = "InKilling" /\ due -> <<Event("ForceKill", n, "")>>
              [] OTHER -> <<>>

GenerateOK(s, a) ==
    /\ a.state = s /\ Values(a.order) = Names /\ Len(a.order) = Cardinality(Names)
    /\ a.reaps = <<>>
    /\ a.events = Flatten([i \in 1..Len(a.order) |->
                            NextEvents(s, a.order[i], a.due[a.order[i]], a.order)])
FinishedOK(s, a) == a.state = s /\ \A n \in Names: s.services[n].status \in Terminal
Init == index = 1 /\ snapshot = Initial /\ valid = (InitialOK /\ SnapshotOK(Initial))
Advance ==
    /\ index <= Len(Actions)
    /\ LET a == Actions[index]
           check == CASE a.kind = "step" -> StepOK(snapshot, a)
                      [] a.kind = "generate" -> GenerateOK(snapshot, a)
                      [] a.kind = "finished" -> FinishedOK(snapshot, a)
                      [] OTHER -> FALSE
       IN /\ valid' = (valid /\ check)
          /\ snapshot' = IF a.kind = "step" THEN a.after ELSE snapshot
    /\ index' = index + 1
Done == index = Len(Actions) + 1 /\ UNCHANGED vars
Next == Advance \/ Done
Spec == Init /\ [][Next]_vars /\ WF_vars(Advance)
Accepted == valid /\ SnapshotOK(snapshot)
Consumed == <>(index = Len(Actions) + 1)
=============================================================================
