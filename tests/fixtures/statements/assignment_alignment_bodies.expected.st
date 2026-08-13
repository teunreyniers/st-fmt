CASE nStep OF
    1:
        nShort          := 1;
        rLongerNameHere := 2.0;
    2:
        nX            := 1;
        nYLongerStill := 2;
END_CASE

IF bEnable THEN
    nThen           := 1;
    rThenLongerName := 2.0;
ELSE
    nElse     := 1;
    rElseName := 2.0;
END_IF

FOR i := 1 TO 10 DO
    nInLoop           := 1;
    rInLoopLongerName := 2.0;
END_FOR
