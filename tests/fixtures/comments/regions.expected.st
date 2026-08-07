FUNCTION_BLOCK FB_Regions
VAR
    {attribute 'symbol' := 'readwrite'}
    /// If true the run is stopped
    StopOnError : BOOL;

    (* Public vars *)

    /// Current state
    {attribute 'symbol' := 'read'}
    State : TestState;
END_VAR

{region Event logic}
above := value >= threshold;
{endregion}
END_FUNCTION_BLOCK
