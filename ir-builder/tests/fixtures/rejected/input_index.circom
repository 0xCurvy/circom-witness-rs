pragma circom 2.0.0;

// An array index that depends on an input has no static address.
template Main() {
    signal input a;
    signal output out;
    var table[4] = [3, 5, 7, 11];
    out <-- table[a];
}

component main = Main();
