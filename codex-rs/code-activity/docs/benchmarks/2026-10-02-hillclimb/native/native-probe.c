#include <tree_sitter/api.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

extern const TSLanguage *tree_sitter_python(void);

static uint64_t now_ns(void) {
    struct timespec value;
    clock_gettime(CLOCK_MONOTONIC, &value);
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}

static TSPoint point_at(const char *source, uint32_t length) {
    TSPoint point = {0, 0};
    for (uint32_t index = 0; index < length; index++) {
        if (source[index] == '\n') {
            point.row++;
            point.column = 0;
        } else {
            point.column++;
        }
    }
    return point;
}

static char *read_source(const char *path, uint32_t *length) {
    FILE *input = fopen(path, "rb");
    if (!input || fseek(input, 0, SEEK_END)) return NULL;
    long size = ftell(input);
    if (size < 0 || size > 1048576 || fseek(input, 0, SEEK_SET)) return NULL;
    char *source = malloc((size_t)size + 1);
    if (!source || fread(source, 1, (size_t)size, input) != (size_t)size) return NULL;
    fclose(input);
    *length = (uint32_t)size;
    source[*length] = 0;
    return source;
}

static TSTree *apply_edit(TSParser *parser, TSTree *old, const char *source,
                          uint32_t length, const char *replacement, uint32_t new_length) {
    uint32_t start = 0, suffix = 0;
    while (start < length && start < new_length && source[start] == replacement[start]) start++;
    while (suffix < length - start && suffix < new_length - start &&
           source[length - suffix - 1] == replacement[new_length - suffix - 1]) suffix++;
    TSInputEdit edit = {
        .start_byte = start, .old_end_byte = length - suffix, .new_end_byte = new_length - suffix,
        .start_point = point_at(source, start),
        .old_end_point = point_at(source, length - suffix),
        .new_end_point = point_at(replacement, new_length - suffix),
    };
    ts_tree_edit(old, &edit);
    TSTree *tree = ts_parser_parse_string(parser, old, replacement, new_length);
    ts_tree_delete(old);
    return tree;
}

static uint64_t hash_bytes(uint64_t hash, const void *bytes, size_t length) {
    const unsigned char *data = bytes;
    for (size_t index = 0; index < length; index++) {
        hash = (hash ^ data[index]) * UINT64_C(1099511628211);
    }
    return hash;
}

/* Walk every syntax node, including comments, anonymous tokens and errors.
 * Tree inspection is outside the parse/destruction timing intervals. */
static uint64_t signature(TSTree *tree, uint32_t *nodes, FILE *dump) {
    TSTreeCursor cursor = ts_tree_cursor_new(ts_tree_root_node(tree));
    uint64_t hash = UINT64_C(14695981039346656037);
    *nodes = 0;
    for (;;) {
        TSNode node = ts_tree_cursor_current_node(&cursor);
        const char *type = ts_node_type(node);
        const char *field = ts_tree_cursor_current_field_name(&cursor);
        TSPoint start = ts_node_start_point(node), end = ts_node_end_point(node);
        uint32_t values[] = {
            ts_node_start_byte(node), ts_node_end_byte(node), start.row, start.column,
            end.row, end.column, ts_node_child_count(node), ts_node_named_child_count(node),
            ts_node_is_named(node), ts_node_is_extra(node), ts_node_is_error(node),
            ts_node_is_missing(node), ts_node_has_error(node),
        };
        hash = hash_bytes(hash, type, strlen(type) + 1);
        hash = hash_bytes(hash, field ? field : "", field ? strlen(field) + 1 : 1);
        hash = hash_bytes(hash, values, sizeof(values));
        if (dump) {
            fprintf(dump, "%s\t%s", type, field ? field : "");
            for (size_t index = 0; index < sizeof(values) / sizeof(values[0]); index++) {
                fprintf(dump, "\t%u", values[index]);
            }
            fputc('\n', dump);
        }
        (*nodes)++;
        if (ts_tree_cursor_goto_first_child(&cursor)) continue;
        while (!ts_tree_cursor_goto_next_sibling(&cursor)) {
            if (!ts_tree_cursor_goto_parent(&cursor)) {
                ts_tree_cursor_delete(&cursor);
                return hash;
            }
        }
    }
}

int main(int argc, char **argv) {
    if (argc < 3 || argc > 5) return 2;
    uint32_t size, new_size = 0;
    char *source = read_source(argv[1], &size);
    char *replacement = argc == 5 ? read_source(argv[4], &new_size) : NULL;
    if (!source || (argc == 5 && !replacement)) return 2;
    unsigned iterations = (unsigned)strtoul(argv[2], NULL, 10);
    if (!iterations || iterations > 1000) return 2;
    TSParser *parser = ts_parser_new();
    if (!ts_parser_set_language(parser, tree_sitter_python())) return 2;
    uint64_t expected = 0;
    printf("{\"iterations\":%u,\"warmup\":1,\"inputBytes\":%u,\"incremental\":%s,\"rawNanoseconds\":[", iterations, size, replacement ? "true" : "false");
    for (unsigned index = 0; index <= iterations; index++) {
        uint64_t before = now_ns();
        TSTree *tree = ts_parser_parse_string(parser, NULL, source, (uint32_t)size);
        if (tree && replacement) tree = apply_edit(parser, tree, source, size, replacement, new_size);
        uint64_t parsed = now_ns();
        if (!tree) return 3;
        uint32_t nodes;
        FILE *dump = argc >= 4 && index == iterations ? fopen(argv[3], "wb") : NULL;
        if (argc >= 4 && index == iterations && !dump) return 2;
        uint64_t observed = signature(tree, &nodes, dump);
        if (dump) fclose(dump);
        bool error = ts_node_has_error(ts_tree_root_node(tree));
        if (index && observed != expected) return 4;
        expected = observed;
        uint64_t dropping = now_ns();
        ts_tree_delete(tree);
        uint64_t elapsed = parsed - before + now_ns() - dropping;
        if (index) printf("%s%" PRIu64, index > 1 ? "," : "", elapsed);
        if (index == iterations) printf("],\"nodes\":%u,\"syntaxError\":%s,\"treeSignature\":\"%016" PRIx64 "\"}\n", nodes, error ? "true" : "false", expected);
    }
    ts_parser_delete(parser);
    free(source);
    free(replacement);
    return 0;
}
