pragma circom 2.0.0;

// A loop bound that depends on an input cannot be unrolled.
template Main() {
    signal input a;
    signal output out;
    var total = 0;
    for (var i = 0; i < a; i++) {
        total += i;
    }
    out <-- total;
}

component main = Main();
