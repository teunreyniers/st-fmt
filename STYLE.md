# st-fmt style

Every rule here was an explicit decision, and every one is pinned by a fixture
under `tests/fixtures/`. Changing a rule means changing the constant, running
`UPDATE_EXPECT=1 cargo test`, and reviewing the diff.

st-fmt is zero-config: there is no config file and no style flags. The constants
live in `src/style.rs`.

- **Width** 100 columns
- **Indent** 4 spaces

## Casing

Structured Text is case-insensitive, so `if`, `If` and `IF` are the same word.

| Thing | Rule | Example |
|---|---|---|
| Keywords | UPPERCASE | `if` → `IF`, `end_function_block` → `END_FUNCTION_BLOCK` |
| Elementary types | UPPERCASE | `bool` → `BOOL`, `int` → `INT` |
| User-defined types | preserved | `FB_Motor`, `analog_event_udt` |
| Identifiers | preserved | `bStart`, `_delay_timer` |
| Operators | UPPERCASE | `mod` → `MOD`, `and` → `AND` |
| Direct addresses | UPPERCASE | `%ix0.0` → `%IX0.0` |
| Strings | never touched | `'Hello World'` |

## Literals

| Form | Rule | Example |
|---|---|---|
| Based integers | hex digits uppercased, separators kept | `16#20fd` → `16#20FD`, `2#1010_1001` unchanged |
| Reals | bare decimal points padded, exponent uppercased | `1.` → `1.0`, `.5` → `0.5`, `1e10` → `1E10` |
| Durations | prefix uppercased, units lowercased | `T#1D2H3M` → `T#1d2h3m` |
| Dates / times | prefix uppercased, body untouched | `d#2026-07-31` → `D#2026-07-31` |
| Typed literals | prefix uppercased | `dint#0` → `DINT#0` |

## Expressions

Access chains are tight; `NOT` takes a space, unary `-` does not.

```
motor.speed     buffer[i]     m[i, j]     items[i]^     p^.field
NOT bFault      -nValue       %IX0.0      THIS^.count
```

**Parentheses are never added or removed**: only the spacing inside them is
normalized. In PLC code parentheses often document intent, and removing them can
only lose information.

**A broken operator chain leads with its operator**, indented one level. Equal
precedence operators are flattened first, so a chain sits at one indent instead
of stair-stepping:

```
bReady := bEnableFromOperatorPanel
    AND (nCycleCountTotal > 10)
    AND NOT bFaultLatchedByGuard
    AND rTemperature < 85.0;
```

## Calls and parameter lists

A call breaks when it does not fit **or** when it has more than two named
parameters. Once broken: one argument per line, `:=` and `=>` aligned in a
common column, a trailing comma, and `)` on its own line.

```
fbShort(a := 1, b := 2);

fbNegated(
    bStart    := TRUE,
    nSpeed    := 100,
    NOT bDone => bReady,
);
```

## Declarations

Declarations align their `AT`, `:` and `:=` columns. **A run is broken only by a
blank line** comments and pragmas stay inside the group, so adding a note never
shifts the column beneath it.

```
VAR
    bStart AT %IX0.0 : BOOL;
    bStop  AT %IX0.1 : BOOL;
    nCount           : INT;
END_VAR
```

Section qualifiers stay on the keyword line: `VAR CONSTANT`,
`VAR RETAIN PERSISTENT`.

Types: `ARRAY [0..9] OF INT` (space before `[`), `STRING[80]` (none). The
parenthesized size form is preserved as written, `STRING(80)` stays, but a
parenthesized *range* is a subrange and takes a space: `INT (0..100)`.

Array initializers pack as many values per line as fit; structure initializers
and enumerations break one per line with their `:=` aligned, on the same
more-than-two rule as call parameters.

```
aBig  : ARRAY [0..19] OF INT := [
    1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000,
];
stCfg : Config               := (
    nFirstSetting  := 1,
    nSecondSetting := 2,
    nThirdSetting  := 3,
);
```

## Control flow

**The trailing keyword breaks with its condition.** `THEN`, `DO` and `OF` drop to
their own line exactly when the condition wraps, never on their own:

```
IF bEnableFromOperatorPanel
    AND (nCycleCountTotal > 10)
    AND rTemperature < 85.0
THEN
    nState := 1;
END_IF
```

**The redundant `;` after a compound statement is removed.** `END_IF;` → `END_IF`.

**A closed block is followed by a blank line.** Once `END_IF`, `END_CASE`,
`END_WHILE`, `END_FOR` or `END_REPEAT` has closed, whatever comes next starts
fresh:

```
IF bFault THEN
    nState := 0;
END_IF

nCycles := nCycles + 1;
```

Two things are deliberately left tight. Nothing is forced after the *last*
statement of a block, where a closing keyword follows rather than another
statement; and nothing is forced before a pragma such as `{endregion}`, which
annotates the code it sits among rather than starting something new. A comment
in the gap keeps the blank line above itself, so it stays attached to the
statement it documents.

**An empty body gets an explicit `;`.** `IF c THEN END_IF` is legal but bare, so
the formatter writes what an author would have typed:

```
IF bCondition THEN
    ;
END_IF
```

CASE labels indent one level from `CASE`, bodies one further; `ELSE` returns to
the `CASE` column:

```
CASE nState OF
    0:
        y := 1;
    1, 2:
        y := 2;
    3..5:
        y := 3;
ELSE
    y := 0;
END_CASE
```

## Program organization units

**Containers stay flat; leaf contents indent.** `VAR`, `STRUCT`, `METHOD` and the
POU keywords all sit at their parent's column, only variable declarations,
struct fields and statements move in a level. Deeply nested ST therefore never
marches off the right margin.

```
FUNCTION_BLOCK PUBLIC FB_Motor EXTENDS FB_Base IMPLEMENTS I_Motor
VAR_INPUT
    bStart : BOOL;
END_VAR

IF bStart THEN
    nState := 1;
END_IF

METHOD PUBLIC Run : BOOL
VAR_INPUT
    x : INT;
END_VAR

Run := TRUE;
END_METHOD

END_FUNCTION_BLOCK
```

`PROPERTY` and `ACTION` are the exception, their contents indent. A property is
a wrapper around its accessors rather than a unit in its own right, and an
action's body is a fragment of the enclosing POU's code, so in both cases the
indentation is what shows where the enclosing POU resumes.

```
PROPERTY Speed : REAL
    GET
        Speed := 1.0;
    END_GET
    SET
        ;
    END_SET
END_PROPERTY

ACTION Reset
    nState := 0;
END_ACTION
```

**A top-level POU closes below a blank line.** `END_PROGRAM`, `END_FUNCTION`,
`END_FUNCTION_BLOCK`, `END_CLASS`, `END_INTERFACE` and
`END_TEST_FUNCTION_BLOCK` are set off from the body above them, so the end of a
long POU reads as a boundary rather than as one more line of code. An empty POU
keeps its keywords together, where a blank line would separate nothing from
nothing. Member terminators (`END_METHOD`, `END_PROPERTY`, `END_ACTION`,
`END_GET`, `END_SET`) stay tight against their bodies.

**Top-level declarations are two blank lines apart.** One POU per screenful is
the normal way to read these files, and a gap wider than any inside a POU is
what makes the boundary unmissable at a glance:

```
Add := a + b;

END_FUNCTION


PROGRAM Main
```

A pragma is the one thing never pushed away: `{attribute 'pack_mode':='1'}`
annotates the declaration beneath it, so it stays on the line above it and the
two blank lines go above the pragma instead.

Blank lines are also **forced** above a POU's statement body and between
members. Never above the first item, and never between a property's `GET` and
`SET`, those are two halves of one declaration. Everywhere else the author's
spacing stands, with runs of blank lines collapsed to one.

## Comments

Comments are `extras` in the grammar, meaning they float to arbitrary positions
in the parse tree, one can even land *inside* an assignment between `:=` and its
right-hand side. Placement is therefore recovered from byte offsets, not tree
shape:

- a comment alone on its line becomes **leading** trivia of what follows
- a comment after code on the same line **trails** it, after two spaces
- a comment between the last declaration and `END_VAR` stays inside the section
- a comment between a pragma and the name it documents stays above the name

A multi-line block comment is copied **byte for byte**. Its interior layout is
the author's, and re-indenting it would corrupt the ASCII tables and diagrams
that are common in PLC headers. Trailing whitespace is still stripped.

Pragmas are copied verbatim: the grammar captures their interior as one opaque
token, so `{attribute 'pack_mode':='1'}` and `{region Event logic}` pass through
untouched.

## Regions

**A region indents what it brackets.** `{region …}` opens a level and
`{endregion}` returns to the opening pragma's column, so the extent of a folded
region is visible even when the editor has it unfolded:

```
{region Event logic}
    IF bTrigger THEN
        nEvents := nEvents + 1;
    END_IF

    {region Diagnostics}
        sLast := 'ok';
    {endregion}
{endregion}
```

This is the one place indentation is not read off the parse tree. To the grammar
the two pragmas are unrelated siblings, so the level is recovered by matching
them while walking a statement list. Only the first word decides: `{region}`,
`{endregion}` and `{end_region}` in any casing, which leaves a region's title
free text and `{attribute …}` unaffected.

Unbalanced markers are formatted, not rejected. An `{endregion}` with nothing
open is left at the level it was found, and a region the author never closed
runs to the end of its block.

A comment sitting above `{endregion}` stays **inside** the region, the same way
a comment above `END_IF` stays inside the block it closes.

## What st-fmt will not do

- **Format an invalid file.** A parse error is a refusal with exit code 2 and no
  output. There is no best-effort mode rewriting a tree containing ERROR nodes
  loses source text.
- **Change a numeric value.** Only casing and padding of literals, never digits.
- **Add or remove parentheses.**
- **Normalize a POU's optional `END_METHOD`.** Whether the author wrote one is
  preserved, so a file that omits them keeps parsing the same way.

## Known limitation

Declaration-only vendor exports, a file ending after the last `END_VAR` with no
`END_FUNCTION_BLOCK`, are refused. The grammar treats them as malformed by
design, because making the terminator optional is ambiguous when several POUs
share a file. Fixing it means changing the grammar, not the formatter.
