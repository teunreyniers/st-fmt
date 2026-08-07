fbMotor(
    bStart := TRUE,
    nSpeed := nSetpoint,
    bDone  => bReady,
    bError => bFault,
);
fbShort(a := 1, b := 2);
fbThree(
    bStart := TRUE,
    nSpeed := 100,
    bDone  => bReady,
);
nResult := ComputeSomethingRatherLong(nFirstOperand, nSecondOperand, nThirdOperand, nFourth);
nOutcome := ComputeAnExtremelyLongFunctionName(
    nFirstOperandValue,
    nSecondOperandValue,
    nThirdOperandValue,
);
fbMixed(
    nPositional,
    bStart := TRUE,
    nSpeed := 100,
    bDone  => bReady,
);
fbNegated(
    bStart    := TRUE,
    nSpeed    := 100,
    NOT bDone => bReady,
);
