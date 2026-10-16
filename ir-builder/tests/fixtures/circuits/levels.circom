pragma circom 2.0.0;

// Levels of different widths, each taking the previous level's whole output array.
template Halve(width) {
    signal input nodes[width];
    signal output parents[width \ 2];
    for (var j = 0; j < width \ 2; j++) {
        parents[j] <== nodes[2 * j] + nodes[2 * j + 1];
    }
}

template Main() {
    signal input leaves[4];
    signal output root;
    component levels[2];
    for (var l = 0; l < 2; l++) {
        levels[l] = Halve(4 >> l);
        if (l == 0) {
            levels[l].nodes <== leaves;
        } else {
            levels[l].nodes <== levels[l - 1].parents;
        }
    }
    root <== levels[1].parents[0];
}

component main = Main();
