/* blockviz: what Prism uses of <locale.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <locale.h>
#else
#pragma once
struct lconv { char *decimal_point; };
static inline struct lconv *localeconv(void) {
    static struct lconv c = { "." };
    return &c;
}
#endif
