for i:=0 to 9 do a[i]:=0; end_for;
FOR i := 0 TO 100 BY 5 DO
sum := sum + i;
END_FOR
while x<10 do
x:=x+1;
end_while;
repeat
x:=x+1;
until x>10
end_repeat;
FOR idx := nStart TO nEnd BY nStep DO
IF a[idx] > 0 THEN
CONTINUE;
END_IF
EXIT;
END_FOR
