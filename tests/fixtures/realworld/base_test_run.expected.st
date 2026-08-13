METHOD Run : BOOL
VAR_INPUT
    duration : TIME := T#0s;
END_VAR
VAR_OUTPUT
    pos : INT;
END_VAR

(*
	Checks the active step and returns true if active.
	Sets the delay if active.
	Calculates the amount of steps in the test.
	Calculates the total amount of delay time for the test.
*)
IF NOT _inizialized THEN
    _error := TRUE;
    SetMessage('Run method called without initialization. Init method needs to be called first.');
END_IF

Run            := CurrentStep = _scan_index AND State = TestState.Running;
OnEntry        := Run AND _step_previous = CurrentStep;
_step_previous := CurrentStep;
pos            := _scan_index;
IF Run THEN
    _delay := duration;
END_IF

_scan_index := _scan_index + 1;
IF State = TestState.Starting AND _scan_index > TotalStepCount THEN
    TotalStepCount := _scan_index;
    IF _loop_index = 0 THEN
        TestDurationExpected := TestDurationExpected + MAX(duration, TO_TIME(_cycle_time));
    ELSE
        _loop_time[_loop_index] := _loop_time[_loop_index] + MAX(duration, TO_TIME(_cycle_time));
    END_IF
END_IF
