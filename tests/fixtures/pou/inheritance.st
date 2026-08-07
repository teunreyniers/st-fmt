FUNCTION_BLOCK public FB_Motor extends FB_Base implements I_Motor, I_Resettable
VAR
n : INT;
END_VAR
n := 1;
END_FUNCTION_BLOCK
INTERFACE I_Motor extends I_Base
METHOD Run : BOOL
END_METHOD
END_INTERFACE
