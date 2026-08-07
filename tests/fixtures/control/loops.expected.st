FOR i := 0 TO 9 DO
    a[i] := 0;
END_FOR
FOR i := 0 TO 100 BY 5 DO
    sum := sum + i;
END_FOR
WHILE x < 10 DO
    x := x + 1;
END_WHILE
REPEAT
    x := x + 1;
UNTIL x > 10
END_REPEAT
FOR idx := nStart TO nEnd BY nStep DO
    IF a[idx] > 0 THEN
        CONTINUE;
    END_IF
    EXIT;
END_FOR
