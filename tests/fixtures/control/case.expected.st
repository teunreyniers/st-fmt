CASE nState OF
    0:
        y := 1;
    1, 2:
        y := 2;
    3..5:
        y := 3;
    Color.Red:
        y := 4;
    -1:
        y := 5;
ELSE
    y := 0;
END_CASE

CASE n OF
    1:
        x := 1;
END_CASE
