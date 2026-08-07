{region Outer}
{attribute 'noinit'}
nCount := 0;
{region Inner logic}
IF bRun THEN
nCount := nCount + 1;
END_IF
// the last word on the inner region
{ENDREGION}

nCount := nCount + 1;
{endregion}
{end_region}
nDone := 1;
{region Never closed}
nLeft := 2;
