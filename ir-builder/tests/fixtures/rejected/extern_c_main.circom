pragma circom 2.1.0;
pragma custom_templates;

// An extern_c main template: circom's generated code calls an external function
// instead of the body written here.

template custom extern_c Foo() {
    signal input a;
    signal output b;
    b <-- a + 1;
}

component main = Foo();
