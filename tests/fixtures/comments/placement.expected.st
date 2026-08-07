/// Doc comment on the POU
FUNCTION_BLOCK FB_Documented
VAR
    n : INT;  // trailing on a declaration
    // a note before END_VAR
END_VAR

// leading the body
n := 1;  // trailing a statement
IF n > 0 THEN
    // inside a branch
    n := 2;
    // before END_IF
END_IF

END_FUNCTION_BLOCK
