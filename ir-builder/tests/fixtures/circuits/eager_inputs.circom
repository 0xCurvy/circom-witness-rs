pragma circom 2.1.0;

// Branches whose condition depends on an input and that feed subcomponent inputs:
// a ternary on the right of an input write (circom turns it into a branch around the
// write), nested branches completing an input written before them, and an `&&` whose
// known operand keeps an out-of-range read off the path.

template Id() {
    signal input in;
    signal output out;
    out <== in;
}

template Pair() {
    signal input in[2];
    signal output out;
    out <== in[0] * 3 + in[1];
}

template EagerInputs(n) {
    signal input x;
    signal input y;
    signal input w[n];
    signal output smallest;
    signal output nested;
    signal output picked[n];

    component smaller = Id();
    smaller.in <-- x < y ? x : y;
    smallest <== smaller.out;

    component pair = Pair();
    pair.in[0] <-- x;
    if (x > y) {
        if (y > 2) {
            pair.in[1] <-- y;
        } else {
            pair.in[1] <-- 2;
        }
    } else {
        pair.in[1] <-- x + y;
    }
    nested <== pair.out;

    var offsets[n];
    for (var i = 0; i < n; i++) {
        offsets[i] = i * 3;
    }
    for (var i = 0; i < n; i++) {
        picked[i] <-- (x > 5 && i + 1 < n) ? w[i + 1] + offsets[i + 1] : w[i];
    }
}

component main = EagerInputs(3);
