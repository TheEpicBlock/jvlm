/*
compile -O3

java_run jvlm.main.main()
expect_timeout 0.1 seconds
*/

// Thread.sleep(long) allows us to test passing arguments into java, without needing to interact with much else
#include <inttypes.h>
#define JLong __uint64_t
void jvlm_extern__java_lang_Thread_sleep(JLong l);

int main() {
    jvlm_extern__java_lang_Thread_sleep(5000);
    return 0;
}