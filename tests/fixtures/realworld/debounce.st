FUNCTION_BLOCK debounce
VAR_INPUT
	input: BOOL;
	debounce_time: TIME;
END_VAR
VAR_OUTPUT
	output: BOOL;
END_VAR
VAR
	_debounce_timer: TON;
END_VAR

_debounce_timer(in := input XOR output, PT := debounce_time);
IF _debounce_timer.Q THEN
	output := input;
ELSE
	output := output;
END_IF;
END_FUNCTION_BLOCK