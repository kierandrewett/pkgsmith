/*
 * A pkgsmith plugin in C. It adds the format "listing": a text file with the name, the version and the path of every
 * file in the package.
 *
 * Build:   cc -shared -fPIC -O2 -o pkgsmith-plugin-listing.so listing.c
 * Install: copy pkgsmith-plugin-listing.so into a folder on the plugin path (see `pkgsmith plugins`), or run
 *          pkgsmith --plugin-path . build
 *
 * The request is JSON. A real plugin should use a JSON library. This example finds the few values it needs with
 * strstr so that it has no dependency. It relies on two facts: `name`, `version` and `out` come before `entries` in
 * the request, and the names and paths in the example do not contain a quote or a backslash.
 */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { uint8_t *ptr; size_t len; } PkgsmithBuf;

typedef struct {
    uint32_t abi_version;
    void *ctx;
    void (*log)(void *ctx, uint32_t level, const uint8_t *message, size_t len);
} PkgsmithHost;

typedef struct {
    uint32_t abi_version;
    PkgsmithBuf (*describe)(void);
    PkgsmithBuf (*call)(const uint8_t *method, size_t method_len, const uint8_t *request, size_t request_len,
                        const PkgsmithHost *host);
    void (*free_buf)(PkgsmithBuf buf);
} PkgsmithPlugin;

static PkgsmithBuf buf_from(const char *text) {
    PkgsmithBuf buf;
    buf.len = strlen(text);
    buf.ptr = malloc(buf.len);
    memcpy(buf.ptr, text, buf.len);
    return buf;
}

static PkgsmithBuf error_reply(const char *message) {
    char text[512];
    snprintf(text, sizeof text, "{\"error\":\"%s\"}", message);
    return buf_from(text);
}

/* Copy the string value after `"key":"` that follows `from`. Returns 0 when it is missing. */
static int json_string(const char *from, const char *key, char *out, size_t size) {
    char pattern[64];
    snprintf(pattern, sizeof pattern, "\"%s\":\"", key);
    const char *start = strstr(from, pattern);
    if (!start) return 0;
    start += strlen(pattern);
    const char *end = strchr(start, '"');
    if (!end || (size_t)(end - start) >= size) return 0;
    memcpy(out, start, end - start);
    out[end - start] = 0;
    return 1;
}

static PkgsmithBuf describe(void) {
    return buf_from("{\"name\":\"listing\",\"version\":\"0.1.0\",\"formats\":[{\"name\":\"listing\","
                    "\"description\":\"A text file that lists the files of the package (.listing)\"}]}");
}

static PkgsmithBuf call(const uint8_t *method, size_t method_len, const uint8_t *request, size_t request_len,
                        const PkgsmithHost *host) {
    char *json = malloc(request_len + 1);
    memcpy(json, request, request_len);
    json[request_len] = 0;

    char name[256], version[256], out[1024], file[1600];
    if (!json_string(json, "name", name, sizeof name) || !json_string(json, "version", version, sizeof version) ||
        !json_string(json, "out", out, sizeof out)) {
        free(json);
        return error_reply("the request has no name, version or out");
    }
    snprintf(file, sizeof file, "%s/%s-%s.listing", out, name, version);

    PkgsmithBuf reply;
    if (method_len == 9 && memcmp(method, "file_name", 9) == 0) {
        char text[1700];
        snprintf(text, sizeof text, "{\"ok\":\"%s-%s.listing\"}", name, version);
        reply = buf_from(text);
    } else if (method_len == 5 && memcmp(method, "build", 5) == 0) {
        FILE *f = fopen(file, "w");
        if (!f) {
            free(json);
            return error_reply("cannot write the listing");
        }
        fprintf(f, "%s %s\n", name, version);
        /* every file entry has a "path":"/..." member */
        for (const char *at = strstr(json, "\"entries\""); at && (at = strstr(at, "\"path\":\"")); ) {
            at += 8;
            const char *end = strchr(at, '"');
            if (!end) break;
            fprintf(f, "%.*s\n", (int)(end - at), at);
        }
        fclose(f);
        if (host && host->log) {
            const char *message = "wrote the listing";
            host->log(host->ctx, 2, (const uint8_t *)message, strlen(message));
        }
        char text[1700];
        snprintf(text, sizeof text, "{\"ok\":\"%s\"}", file);
        reply = buf_from(text);
    } else {
        reply = error_reply("unknown method");
    }
    free(json);
    return reply;
}

static void free_buf(PkgsmithBuf buf) { free(buf.ptr); }

static const PkgsmithPlugin PLUGIN = {1, describe, call, free_buf};

const PkgsmithPlugin *pkgsmith_plugin_entry(void) { return &PLUGIN; }
