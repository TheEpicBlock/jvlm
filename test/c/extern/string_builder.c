/*
compile -O3

java_run jvlm.test.test()
expect 00
*/

#include <inttypes.h>
#define JInt __int32_t

typedef struct java_stringbuilder __attribute__((address_space(1))) * StringBuilder;

StringBuilder jvlm_extern_new__java_lang_StringBuilder();
void jvlm_extern_invokespecial__java_lang_StringBuilder_\u022Ainit\u022B(StringBuilder this);
StringBuilder jvlm_extern_invokevirtual__java_lang_StringBuilder_append$jvlm_param$java_lang_StringBuilder(StringBuilder this, JInt a);

StringBuilder test$jvlm_param$java_lang_StringBuilder() {
    // Create and initialize a string builder
    StringBuilder builder = jvlm_extern_new__java_lang_StringBuilder();
    jvlm_extern_invokespecial__java_lang_StringBuilder_\u022Ainit\u022B(builder);

    // Append two zeros to the string builder
    jvlm_extern_invokevirtual__java_lang_StringBuilder_append$jvlm_param$java_lang_StringBuilder(builder, 0);
    jvlm_extern_invokevirtual__java_lang_StringBuilder_append$jvlm_param$java_lang_StringBuilder(builder, 0);
    return builder;
}