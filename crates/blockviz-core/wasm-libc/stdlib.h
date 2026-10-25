/* blockviz: what Prism uses of <stdlib.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <stdlib.h>
#else
#pragma once
#include <stddef.h>

/* The allocator is Rust's (src/ruby/libc.rs), as is strtod. */
void *malloc(size_t size);
void *calloc(size_t count, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);
double strtod(const char *s, char **end);
_Noreturn void abort(void);

/* Decimal only (what Prism asks for); too large a number is the largest. */
static inline unsigned long strtoul(const char *s, char **end, int base) {
    unsigned long value = 0;
    for (; *s >= '0' && *s <= '9'; s++) {
        unsigned long digit = (unsigned long) (*s - '0');
        value = value > (~0UL - digit) / (unsigned long) base ? ~0UL : value * (unsigned long) base + digit;
    }
    if (end) *end = (char *) s;
    return value;
}

/* Prism sorts a file's comments, which arrive nearly in order: insertion sort. */
static inline void qsort(void *base, size_t count, size_t size, int (*compare)(const void *, const void *)) {
    unsigned char *items = (unsigned char *) base;
    unsigned char tmp[16];
    if (size > sizeof(tmp)) abort();
    for (size_t i = 1; i < count; i++) {
        for (size_t j = i; j > 0 && compare(items + (j - 1) * size, items + j * size) > 0; j--) {
            unsigned char *a = items + (j - 1) * size, *b = items + j * size;
            for (size_t k = 0; k < size; k++) { tmp[k] = a[k]; a[k] = b[k]; b[k] = tmp[k]; }
        }
    }
}
#endif
