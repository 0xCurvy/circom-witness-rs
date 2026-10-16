pragma circom 2.1.0;

// circom's semantics where evaluating both sides of a select, or a plain field
// operation, would fail or differ: a division hoisted out of a ternary, a call and an
// `&&` (circom moves nested ternaries and calls out of their arm and does not
// short-circuit, so the division runs on the untaken path too); shifts by any amount,
// known or not; a function named like a black box; a function that returns fewer
// values than its destination holds; and a divisor that only the optimizer sees is 0.

function bbf_inv(v) {
    return v + 1;
}

function short(c) {
    var a[2] = [1, 2];
    var b[3] = [4, 5, 6];
    if (c) {
        return a;
    }
    return b;
}

function sq(v) {
    return v * v;
}

template Semantics() {
    signal input x;
    signal input y;
    signal input amount;
    signal output hoisted[4];
    signal output shifted[4];
    signal output folded[3];
    signal output misc[3];

    hoisted[0] <-- x == 0 ? 1 : (y == 0 ? 2 : y / x);
    hoisted[1] <-- x != 0 ? sq(1 / x) : 0;
    hoisted[2] <-- (x != 0) && (5 / x > 1);
    hoisted[3] <-- y / x;

    shifted[0] <-- y << amount;
    shifted[1] <-- y >> amount;
    shifted[2] <-- 12345 << amount;
    shifted[3] <-- y >> (0 - amount);

    var minus_one = 0;
    for (var i = 0; i < 1; i++) {
        minus_one = minus_one - 1;
    }
    folded[0] <-- y << 300;
    folded[1] <-- y >> minus_one;
    folded[2] <-- 7 << minus_one;

    var z[3] = short(1);
    misc[0] <-- bbf_inv(y);
    misc[1] <-- z[0] + z[1] * 10 + z[2] * 100 + x;
    misc[2] <-- 5 / ((x == x) - 1);
}

component main = Semantics();
