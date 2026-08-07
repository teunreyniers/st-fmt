CONFIGURATION Plant
VAR_GLOBAL
    gCounter : INT;
END_VAR
RESOURCE Station1 ON CPU001
VAR_GLOBAL
    gLocal : BOOL;
END_VAR
TASK Fast(interval := T#10ms, priority := 1);
TASK Slow(interval := T#1s, priority := 5);
PROGRAM Main WITH Fast : MainProg;
PROGRAM Aux WITH Slow : AuxProg(nInput := 1, bFlag => bResult);
END_RESOURCE
END_CONFIGURATION
