/*
compile -O3

java_run jvlm.ternary.ternary(5)
expect 34
java_run jvlm.ternary.ternary(6)
expect 15
*/

int ternary(int a) {
    return a > 5 ? 15 : 34;
}