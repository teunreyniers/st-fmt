a := MAX(x,y);
b := INT_TO_REAL( x );
c := FPU.IsRealNumber(x);
d := fb.Run(x := 1);
e := Compute(a, b, c, d, e, f);
f := NoArgs();
fbPump(bStart := TRUE, nSpeed := 100);
