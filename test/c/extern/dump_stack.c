/*
compile -O3

java_run jvlm.main.main()
expect_contains at java.base/java.lang.Thread.dumpStack
*/

// Thread.dumpStack() is a really convenient function which doesn't require passing/receiving anything
// but does output really recognizeable stuff onto stdout

void jvlm_extern__java_lang_Thread_dumpStack();

int main() {
    jvlm_extern__java_lang_Thread_dumpStack();
    return 0;
}