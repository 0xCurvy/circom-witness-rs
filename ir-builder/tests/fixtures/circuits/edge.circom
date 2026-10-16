pragma circom 2.2.0;

// Buses, nested buses and a three-dimensional array in a component array whose
// elements are different template instances, so circom addresses their IO through
// its IO table; partial-array stores; recursive functions and an early return from
// a loop; an anonymous component; and input-dependent selects inside a loop.

bus Point() {
    signal x;
    signal y;
}

bus Segment() {
    Point() ends[2];
    signal weight;
}

template Shape(rows, cols, k) {
    input Segment() segs[rows];
    signal input grid[rows][cols][2];
    output Point() out[rows];
    signal output sums[rows][cols];
    for (var i = 0; i < rows; i++) {
        out[i].x <== segs[i].ends[0].x * k + segs[i].ends[1].y;
        out[i].y <== segs[i].weight + grid[i][0][1];
        for (var j = 0; j < cols; j++) {
            sums[i][j] <== grid[i][j][0] * (k + j) + grid[i][j][1];
        }
    }
}

template Double() {
    signal input in;
    signal output out;
    out <== in * 2;
}

function tri(n) {
    if (n == 0) {
        return 0;
    }
    return n + tri(n - 1);
}

function firstAbove(values, n, limit) {
    for (var i = 0; i < n; i++) {
        if (values[i] > limit) {
            return i;
        }
    }
    return n;
}

template Edge() {
    signal input v[12];
    signal output o[16];
    signal output f[3];
    signal output anon;
    signal output inv[3];

    signal pairs[2][3][2];
    for (var i = 0; i < 2; i++) {
        for (var j = 0; j < 3; j++) {
            pairs[i][j][0] <== v[(i + j) % 12];
            pairs[i][j][1] <== v[(i + 2 * j + 5) % 12];
        }
    }

    component s[2];
    for (var k = 0; k < 2; k++) {
        s[k] = Shape(2, 2 + k, k + 2);
        for (var i = 0; i < 2; i++) {
            s[k].segs[i].ends[0].x <== v[i];
            s[k].segs[i].ends[0].y <== v[i + 1];
            s[k].segs[i].ends[1].x <== v[i + 2];
            s[k].segs[i].ends[1].y <== v[i + 3];
            s[k].segs[i].weight <== v[i + 4];
            for (var j = 0; j < 2 + k; j++) {
                s[k].grid[i][j] <== pairs[i][j];
            }
        }
    }
    for (var k = 0; k < 2; k++) {
        for (var i = 0; i < 2; i++) {
            o[k * 8 + i * 4 + 0] <== s[k].out[i].x;
            o[k * 8 + i * 4 + 1] <== s[k].out[i].y;
            o[k * 8 + i * 4 + 2] <== s[k].sums[i][0];
            o[k * 8 + i * 4 + 3] <== s[k].sums[i][1 + k];
        }
    }

    var limits[4] = [3, 9, 1, 12];
    f[0] <-- tri(5) + v[0];
    f[1] <-- firstAbove(limits, 4, 8) * v[1];
    f[2] <-- v[2] * tri(firstAbove(limits, 4, 10));

    anon <== Double()(v[5]);

    for (var i = 0; i < 3; i++) {
        inv[i] <-- v[i + 6] != 0 ? 1 / v[i + 6] : 0;
    }
}

component main = Edge();
