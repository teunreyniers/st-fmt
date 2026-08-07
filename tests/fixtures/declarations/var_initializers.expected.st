VAR_GLOBAL
    aSmall  : ARRAY [0..2] OF INT  := [1, 2, 3];
    aRepeat : ARRAY [0..9] OF INT  := [10(0)];
    aBig    : ARRAY [0..19] OF INT := [
        1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000, 11000, 12000, 13000, 14000,
    ];
    stPoint : Point                := (x := 1, y := 2);
    stWide  : Config               := (
        nFirstSetting  := 1,
        nSecondSetting := 2,
        nThirdSetting  := 3,
        nFourth        := 4,
    );
    sText   : STRING[80]           := 'hello';
END_VAR
