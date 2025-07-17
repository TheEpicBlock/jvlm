/*
compile -O3

java_run jvlm.intTest.intTest(1, 1)
expect 4
java_run jvlm.intTest.intTest(2, 5)
expect 49
*/

int intTest(int a, int b) {
    int c = a + b;
    // Note how the result of an operation gets used twice here
    return c * c;
}