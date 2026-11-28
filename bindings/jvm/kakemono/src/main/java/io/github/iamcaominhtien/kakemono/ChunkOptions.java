package io.github.iamcaominhtien.kakemono;

/** Options of the chunks for retrieval. */
public final class ChunkOptions extends Options<ChunkOptions> {
    /** Options with kakemono's defaults. */
    public ChunkOptions() {}

    /** Largest chunk, in tokens (default 512). */
    public ChunkOptions maxTokens(int maxTokens) {
        return set("maxTokens", maxTokens);
    }

    /** The path of a Hugging Face {@code tokenizer.json}, or {@code cl100k_base} / {@code o200k_base}; tokens are estimated without it. */
    public ChunkOptions tokenizer(String tokenizer) {
        return set("tokenizer", tokenizer);
    }
}
