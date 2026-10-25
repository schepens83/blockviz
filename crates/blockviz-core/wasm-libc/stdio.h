/* blockviz: what Prism uses of <stdio.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <stdio.h>
#else
#pragma once
#include <stdarg.h>
#include <stddef.h>

/* Nothing is printed: Prism writes to stderr only in debugging code and just
   before aborting. Macros, so the compiler cannot turn them into calls to
   fwrite. */
#define fprintf(...) ((void) 0)
#define fputs(s, f) ((void) 0)
#define perror(s) ((void) 0)

/* Diagnostic messages: %s, %.*s, %c, %d, %i and %x/%X with flags, width,
   precision and the l, ll, z length modifiers. */
static inline void pm_wasm_put(char *out, size_t size, size_t *n, char c) {
    if (*n + 1 < size) out[*n] = c;
    (*n)++;
}

static inline int vsnprintf(char *out, size_t size, const char *format, va_list args) {
    size_t n = 0;
    for (const char *f = format; *f; f++) {
        if (*f != '%') { pm_wasm_put(out, size, &n, *f); continue; }
        f++;
        int left = 0, zero = 0;
        for (;; f++) {
            if (*f == '-') left = 1;
            else if (*f == '0') zero = 1;
            else break;
        }
        int width = 0;
        if (*f == '*') { width = va_arg(args, int); f++; }
        else while (*f >= '0' && *f <= '9') width = width * 10 + (*f++ - '0');
        int precision = -1;
        if (*f == '.') {
            f++;
            precision = 0;
            if (*f == '*') { precision = va_arg(args, int); f++; }
            else while (*f >= '0' && *f <= '9') precision = precision * 10 + (*f++ - '0');
        }
        int longs = 0, sized = 0;
        while (*f == 'l') { longs++; f++; }
        if (*f == 'z') { sized = 1; f++; }

        char digits[24];
        const char *text = digits;
        int length = 0;
        switch (*f) {
            case '%': pm_wasm_put(out, size, &n, '%'); continue;
            case 'c': digits[0] = (char) va_arg(args, int); length = 1; break;
            case 's': {
                text = va_arg(args, const char *);
                if (!text) text = "(null)";
                while ((precision < 0 || length < precision) && text[length]) length++;
                break;
            }
            case 'd': case 'i': case 'u': case 'x': case 'X': {
                unsigned long long value;
                int negative = 0;
                if (*f == 'd' || *f == 'i') {
                    long long v = sized ? (long long) va_arg(args, long) : longs >= 2 ? va_arg(args, long long) : longs ? va_arg(args, long) : va_arg(args, int);
                    negative = v < 0;
                    value = negative ? 0ULL - (unsigned long long) v : (unsigned long long) v;
                } else {
                    value = sized ? va_arg(args, size_t) : longs >= 2 ? va_arg(args, unsigned long long) : longs ? va_arg(args, unsigned long) : va_arg(args, unsigned int);
                }
                const char *alphabet = *f == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
                unsigned base = (*f == 'x' || *f == 'X') ? 16 : 10;
                char reversed[24];
                int count = 0;
                do { reversed[count++] = alphabet[value % base]; value /= base; } while (value);
                if (negative) digits[length++] = '-';
                for (int pad = (zero && !left) ? width - count - negative : 0; pad > 0; pad--) digits[length++] = '0';
                while (count) digits[length++] = reversed[--count];
                break;
            }
            default: return -1;
        }
        if (!left) for (int pad = width - length; pad > 0; pad--) pm_wasm_put(out, size, &n, ' ');
        for (int i = 0; i < length; i++) pm_wasm_put(out, size, &n, text[i]);
        if (left) for (int pad = width - length; pad > 0; pad--) pm_wasm_put(out, size, &n, ' ');
    }
    if (size) out[n < size ? n : size - 1] = '\0';
    return (int) n;
}
#endif
