pragma circom 2.1.0;

// A read past the last signal on one side of a branch whose condition depends only on
// an input. circom's generated code reads it only where that side is taken; the graph
// computes the other side's value where it is not, and fails where it is.

template OutOfRange(n) {
    signal output o[n];
    signal input x;
    signal input w[n];
    for (var i = 0; i < n; i++) {
        o[i] <-- x > 5 ? w[i + 1] : w[i];
    }
}

component main = OutOfRange(3);
