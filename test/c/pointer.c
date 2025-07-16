__attribute__((noinline))
void set(int* a) {
    *a = 1;
}

int test() {
    int num;
    set(&num);
    return num;
}