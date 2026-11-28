package io.github.iamcaominhtien.kakemono;

import java.util.List;
import java.util.Optional;

/**
 * A chunk for retrieval (RAG), cut at sentence ends.
 *
 * @param id number of the chunk, in reading order, from 0
 * @param text the text to embed: the section path, a blank line, then the text
 * @param tokens its size in tokens
 * @param title the title of the document
 * @param section the headings of the section it is in, from the outermost
 * @param blocks the blocks of the document it holds (their numbers)
 * @param pages the pages it comes from, from 0 ({@code [0]} for documents without pages)
 */
public record Chunk(
        int id, String text, int tokens, Optional<String> title, List<String> section, List<Integer> blocks,
        List<Integer> pages) {}
