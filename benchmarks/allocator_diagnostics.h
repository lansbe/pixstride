/* Diagnostic adapter only; never included by the Pixstride service. */
#pragma once
#include <stdbool.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <gnu/libc-version.h>
#include <sys/prctl.h>
#ifdef PROFILE_GLIBC
#include <malloc.h>
/* Reuse the primitive driver with two explicit diagnostic API adapters. */
#define MIMALLOC_H
static int mi_version(void) {
    int major = 0, minor = 0;
    if (sscanf(gnu_get_libc_version(), "%d.%d", &major, &minor) != 2) exit(5);
    return major * 10000 + minor;
}
static void mi_collect(bool force) { if (force) malloc_trim(0); }
#endif
__attribute__((constructor)) static void allocator_diagnostic_setup(void) {
    const char *setting = getenv("PROFILE_DISABLE_THP");
    if (setting && !strcmp(setting, "1") && prctl(PR_SET_THP_DISABLE, 1L, 0L, 0L, 0L)) {
        fprintf(stderr, "Cannot disable THP in diagnostic process\n");
        exit(5);
    }
}
static int allocator_diagnostic_printf(const char *format, ...) {
    va_list args;
    va_start(args, format);
    int written = vprintf(format, args);
    va_end(args);
    unsigned long long huge = 0;
    bool found = false;
    FILE *maps = fopen("/proc/self/smaps_rollup", "r");
    if (!maps) exit(5);
    char line[256];
    while (fgets(line, sizeof(line), maps)) {
        if (sscanf(line, "AnonHugePages: %llu kB", &huge) == 1) { found = true; break; }
    }
    fclose(maps);
    if (!found) exit(5);
    unsigned long long resident_peak = 0, anonymous = 0;
    FILE *status = fopen("/proc/self/status", "r");
    if (!status) exit(5);
    while (fgets(line, sizeof(line), status)) {
        if (sscanf(line, "VmHWM: %llu kB", &resident_peak) == 1) continue;
        if (sscanf(line, "RssAnon: %llu kB", &anonymous) == 1) continue;
    }
    fclose(status);
    if (!resident_peak || !anonymous) exit(5);
    int disabled = prctl(PR_GET_THP_DISABLE, 0L, 0L, 0L, 0L);
#ifdef PROFILE_GLIBC
    const char *allocator = "glibc";
#else
    const char *allocator = "mimalloc";
#endif
    fprintf(stderr, "ALLOCATOR_METADATA {\"allocator\":\"%s\",\"libc_version\":\"%s\","
        "\"thp_disabled\":%d,\"anonymous_huge_bytes\":%llu,"
        "\"process_peak_rss_bytes\":%llu,\"process_anonymous_bytes\":%llu}\n",
        allocator, gnu_get_libc_version(), disabled, huge * 1024, resident_peak * 1024, anonymous * 1024);
    return written;
}
#define printf allocator_diagnostic_printf
