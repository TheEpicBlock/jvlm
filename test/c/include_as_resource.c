/*
compile -O3
*/
// TODO add test

#include <inttypes.h>

__attribute((annotate("jvlm::include_as_resource(/bin/test)")))
const uint8_t data[] = {
    0x1, 0x2, 0x3
};