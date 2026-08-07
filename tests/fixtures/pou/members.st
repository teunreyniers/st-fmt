FUNCTION_BLOCK FB_Motor
VAR
nState : INT;
END_VAR
nState := 0;
method public Run : bool
var_input
x : int;
end_var
Run := TRUE;
end_method
property Speed : REAL
get
Speed := 1.0;
end_get
set
;
end_set
end_property
action Reset
nState := 0;
end_action
END_FUNCTION_BLOCK
