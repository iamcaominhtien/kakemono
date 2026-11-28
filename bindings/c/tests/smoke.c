/* The C ABI from C: converts a document to Markdown and prints its first line.
 *   cc -I include tests/smoke.c -L ../target/release -lkakemono_ffi -o smoke
 *   LD_LIBRARY_PATH=../target/release ./smoke ../../tests/data/word/sample.docx */
#include <stdio.h>
#include <string.h>
#include "kakemono.h"

int main(int argc, char **argv) {
    if (argc != 2 || kk_abi_version() != KK_ABI_VERSION) {
        fprintf(stderr, "usage: smoke FILE (ABI %u)\n", kk_abi_version());
        return 2;
    }
    KkError *err = NULL;
    KkDocument *doc = kk_open_path(argv[1], "{\"jobs\":1}", &err);
    if (!doc) {
        fprintf(stderr, "error %u: %s\n", kk_error_kind(err), kk_error_message(err, NULL));
        kk_error_free(err);
        return 1;
    }
    KkOutput *out = kk_render(doc, "markdown", "{\"inlineImages\":true}", &err);
    if (!out) {
        fprintf(stderr, "error %u: %s\n", kk_error_kind(err), kk_error_message(err, NULL));
        kk_error_free(err);
        kk_document_free(doc);
        return 1;
    }
    size_t len = 0;
    const char *text = kk_output_text(out, &len);
    const char *end = memchr(text, '\n', len);
    printf("%s %s: %.*s\n", kk_version(NULL), kk_document_format(doc, NULL), (int)(end ? end - text : (long)len), text);
    kk_output_free(out);
    kk_document_free(doc);
    return 0;
}
