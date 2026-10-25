/* blockviz: what Prism uses of <string.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <string.h>
#else
#pragma once
#include <stddef.h>

void *memcpy(void *dest, const void *src, size_t n);
void *memmove(void *dest, const void *src, size_t n);
void *memset(void *s, int c, size_t n);
int memcmp(const void *a, const void *b, size_t n);

static inline void *memchr(const void *s, int c, size_t n) {
    const unsigned char *p = (const unsigned char *) s;
    for (; n > 0; n--, p++) if (*p == (unsigned char) c) return (void *) p;
    return NULL;
}

static inline size_t strlen(const char *s) {
    const char *p = s;
    while (*p) p++;
    return (size_t) (p - s);
}

static inline char *strchr(const char *s, int c) {
    for (;; s++) {
        if (*s == (char) c) return (char *) s;
        if (!*s) return NULL;
    }
}

static inline int strncmp(const char *a, const char *b, size_t n) {
    for (; n > 0; n--, a++, b++) {
        if (*a != *b) return (unsigned char) *a - (unsigned char) *b;
        if (!*a) return 0;
    }
    return 0;
}
#endif
