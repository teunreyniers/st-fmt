configuration Plant
VAR_GLOBAL
gCounter : INT;
END_VAR
resource Station1 on CPU001
VAR_GLOBAL
gLocal : BOOL;
END_VAR
task Fast(interval := t#10ms, priority := 1);
task Slow(interval := T#1S, priority:=5);
program Main with Fast : MainProg;
program Aux with Slow : AuxProg(nInput := 1, bFlag => bResult);
end_resource
end_configuration
