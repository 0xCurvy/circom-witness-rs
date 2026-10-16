pragma circom 2.0.0;

// A component array whose elements are different template instances, so circom
// addresses their IO through the per-template IO map ("mapped" locations).
template Grid(rows, cols) {
    signal input in[rows][cols];
    signal output out[rows][cols];
    for (var i = 0; i < rows; i++) {
        for (var j = 0; j < cols; j++) {
            out[i][j] <== in[i][j] * (i * 10 + j + 1);
        }
    }
}

template Main() {
    signal input a[2][3];
    signal output o0[2][3];
    signal output o1[2][2];
    component g[2];
    for (var k = 0; k < 2; k++) {
        g[k] = Grid(2, 3 - k);
        for (var i = 0; i < 2; i++) {
            for (var j = 0; j < 3 - k; j++) {
                g[k].in[i][j] <== a[i][j];
            }
        }
    }
    for (var i = 0; i < 2; i++) {
        for (var j = 0; j < 3; j++) {
            o0[i][j] <== g[0].out[i][j];
        }
        for (var j = 0; j < 2; j++) {
            o1[i][j] <== g[1].out[i][j];
        }
    }
}

component main = Main();
