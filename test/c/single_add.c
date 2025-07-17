/*
compile -O3

java_run jvlm.intTest.intTest(0, 0)
expect 0
java_run jvlm.intTest.intTest(1,2)
expect 3
*/

int intTest(int a, int b) {
    return a + b;
}