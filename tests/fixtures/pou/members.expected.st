FUNCTION_BLOCK FB_Motor
VAR
    nState : INT;
END_VAR

nState := 0;

METHOD PUBLIC Run : BOOL
VAR_INPUT
    x : INT;
END_VAR

Run := TRUE;
END_METHOD

PROPERTY Speed : REAL
    GET
        Speed := 1.0;
    END_GET
    SET
        ;
    END_SET
END_PROPERTY

ACTION Reset
    nState := 0;
END_ACTION

END_FUNCTION_BLOCK
