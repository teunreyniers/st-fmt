type
Point : struct
x : real;
y : real;
label : STRING[32] := '';
end_struct

Meters : INT (0..100);
Alias : ARRAY [0..9] OF int;
Color : (Red, Green, Blue);
State : (Idle := 0, Running := 10, Faulted := 20);
Counter : udint := 0;
end_type
