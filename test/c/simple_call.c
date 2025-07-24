/*
compile

java_run jvlm.test.test()
expect 123
*/

// TODO this is shaky. It relies on not being optimized in order to not inline this. Maybe we should just test on the raw LL file?

__attribute ((noinline))
int testInner() {
    return 123;
}

int test() {
    return testInner();
}
