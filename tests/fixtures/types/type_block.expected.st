TYPE
Point : STRUCT
    x     : REAL;
    y     : REAL;
    label : STRING[32] := '';
END_STRUCT

Meters : INT (0..100);
Alias : ARRAY [0..9] OF INT;
Color : (Red, Green, Blue);
State : (
    Idle    := 0,
    Running := 10,
    Faulted := 20,
);
Counter : UDINT := 0;
END_TYPE
