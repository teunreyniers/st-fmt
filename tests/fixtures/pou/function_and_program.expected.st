FUNCTION Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR

Add := a + b;

END_FUNCTION


PROGRAM Main
VAR
    x : INT;
END_VAR

x := Add(1, 2);

END_PROGRAM
