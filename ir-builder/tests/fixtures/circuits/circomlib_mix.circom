pragma circom 2.0.0;

// circomlib as Curvy's circuits use it, unpatched: IsZero's input-dependent inverse,
// comparators, bit decomposition, Poseidon, multiplexers, Baby Jubjub addition and
// SHA-256.
include "circomlib/circuits/comparators.circom";
include "circomlib/circuits/bitify.circom";
include "circomlib/circuits/poseidon.circom";
include "circomlib/circuits/mux1.circom";
include "circomlib/circuits/switcher.circom";
include "circomlib/circuits/babyjub.circom";
include "circomlib/circuits/sha256/sha256.circom";

template CircomlibMix() {
    signal input a;
    signal input b;
    signal input selector;
    signal input point[4];
    signal input message[16];
    signal output zero;
    signal output equal;
    signal output less;
    signal output bits[16];
    signal output hash;
    signal output muxed;
    signal output switched[2];
    signal output sum[2];
    signal output digest[256];

    zero <== IsZero()(a - b);
    equal <== IsEqual()([a, b]);
    less <== LessThan(32)([a, b]);
    bits <== Num2Bits(16)(a);
    hash <== Poseidon(3)([a, b, selector]);
    muxed <== Mux1()([a, b], selector);
    component switcher = Switcher();
    switcher.sel <== selector;
    switcher.L <== a;
    switcher.R <== b;
    switched[0] <== switcher.outL;
    switched[1] <== switcher.outR;
    component add = BabyAdd();
    add.x1 <== point[0];
    add.y1 <== point[1];
    add.x2 <== point[2];
    add.y2 <== point[3];
    sum[0] <== add.xout;
    sum[1] <== add.yout;
    digest <== Sha256(16)(message);
}

component main = CircomlibMix();
