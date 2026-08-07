IF bEnableFromOperatorPanel
    AND (nCycleCountTotal > 10)
    AND NOT bFaultLatchedByGuard
    AND rTemperatureMeasured < 85.0
THEN
    nState := 1;
END_IF

WHILE bStillRunningTheProcess
    AND nRemainingItemCount > 0
    AND NOT bAbortRequestedByUser
    AND bConveyorBeltIsMoving
DO
    nRemainingItemCount := nRemainingItemCount - 1;
END_WHILE
