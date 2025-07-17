/*
compile -O3

java_run jvlm.test.test()
expect 1
*/

__attribute__((noinline))
void set(int* a) {
    *a = 1;
}

int test() {
    int num;
    set(&num);
    return num;
}