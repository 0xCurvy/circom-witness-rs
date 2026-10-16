pragma circom 2.0.0;

// Every operator, input-dependent selects and functions, computed with `<--` so any
// input is a valid witness input.

function poly(x, n) {
    var acc = 0;
    var power = 1;
    for (var i = 0; i < n; i++) {
        acc += power * (i + 1);
        power *= x;
    }
    return acc;
}

function pair(a, b) {
    var r[2];
    r[0] = a + b;
    r[1] = a * b;
    return r;
}

template IsZeroLike() {
    signal input in;
    signal output out;
    signal inv;
    inv <-- in != 0 ? 1 / in : 0;
    out <== -in * inv + 1;
    in * out === 0;
}

template Ops() {
    signal input a;
    signal input b;
    signal input c[3];
    signal output arith[6];
    signal output cmp[6];
    signal output bits[9];
    signal output misc[6];

    arith[0] <-- a + b;
    arith[1] <-- a - b;
    arith[2] <-- a * b;
    arith[3] <-- b != 0 ? a / b : 0;
    arith[4] <-- a ** 3;
    arith[5] <-- -a;

    cmp[0] <-- a < b;
    cmp[1] <-- a > b;
    cmp[2] <-- a <= b;
    cmp[3] <-- a >= b;
    cmp[4] <-- a == b;
    cmp[5] <-- a != b;

    bits[0] <-- a & b;
    bits[1] <-- a | b;
    bits[2] <-- a ^ b;
    bits[3] <-- ~a;
    bits[4] <-- (a & 255) << 3;
    bits[5] <-- a >> 7;
    bits[6] <-- b != 0 ? a \ b : 0;
    bits[7] <-- b != 0 ? a % b : 0;
    bits[8] <-- (a & 15) >> (b & 7);

    misc[0] <-- (a < b) || (b == 0);
    misc[1] <-- (a < b) && !(b == 0);
    misc[2] <-- poly(a, 4);
    var r[2] = pair(a, b);
    misc[3] <-- r[0] * r[1];
    var t = 0;
    if (a < b) {
        if (c[0] == 0) {
            t = 1;
        } else {
            t = 2;
        }
    } else {
        t = b != 0 ? a / b : 7;
    }
    misc[4] <-- t;
    component z = IsZeroLike();
    z.in <== c[1] - c[2];
    misc[5] <== z.out;
}

component main = Ops();
