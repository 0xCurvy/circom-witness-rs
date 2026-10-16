pragma circom 2.0.0;

// A function branch that depends on an input cannot be selected eagerly.
function pick(x) {
    if (x == 0) {
        return 7;
    }
    return x;
}

template Main() {
    signal input a;
    signal output out;
    out <-- pick(a);
}

component main = Main();
