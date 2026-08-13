nState         := 1;
rSpeedSetpoint := 12.5;
bEnable        := TRUE;

nOther := 0;
sName  := 'x';

nFirst := 1;
fbPump();
nSecond := 2;

nLead         := 1;
// a note in the middle
nFollowerName := 2;

pTarget REF= pSource;
nState  :=   1;
rSpeed  :=   12.5;

motor.speed := 1;
buffer[i]   := 2;
p^.field    := 3;

nAttributed      := 1;
{attribute 'note'}
rAttributedValue := 2.0;

IF bEnable THEN
    nInner      := 1;
    rInnerValue := 2.0;
END_IF

nAfterCompound := 1;

{region Setup}
    nInRegion      := 1;
    rInRegionValue := 2.0;
{endregion}
