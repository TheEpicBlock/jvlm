int stackTest(int a, int b, int c) {
    // This test is somewhat interesting to translate from SSA to stack
    int d = a + b;
    int e = d * c;
    int f = e * d;
    return f;
}