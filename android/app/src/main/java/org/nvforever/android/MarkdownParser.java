package org.nvforever.android;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/** Pure-Java subset Markdown parser for GitHub news and release notes. */
final class MarkdownParser {
    enum BlockKind { HEADING, PARAGRAPH, BULLET, ORDERED, QUOTE, CODE, RULE }
    enum SpanKind { TEXT, BOLD, ITALIC, BOLD_ITALIC, CODE, STRIKE, LINK }

    static final class Span {
        final SpanKind kind;
        final String text;
        final String url;

        Span(SpanKind kind, String text, String url) {
            this.kind = kind;
            this.text = text;
            this.url = url;
        }
    }

    static final class Block {
        final BlockKind kind;
        final int level;
        final String prefix;
        final List<Span> spans;

        Block(BlockKind kind, int level, String prefix, List<Span> spans) {
            this.kind = kind;
            this.level = level;
            this.prefix = prefix;
            this.spans = Collections.unmodifiableList(spans);
        }

        String plainText() {
            StringBuilder out = new StringBuilder();
            if (!prefix.isEmpty()) out.append(prefix);
            for (Span span : spans) out.append(span.text);
            return out.toString();
        }
    }

    static final class Preview {
        final List<Block> blocks;
        final boolean truncated;

        Preview(List<Block> blocks, boolean truncated) {
            this.blocks = Collections.unmodifiableList(blocks);
            this.truncated = truncated;
        }
    }

    private MarkdownParser() {}

    static List<Block> parse(String markdown) {
        List<Block> out = new ArrayList<>();
        if (markdown == null || markdown.isEmpty()) return out;
        String[] lines = markdown.replace("\r\n", "\n").replace('\r', '\n').split("\n", -1);
        StringBuilder paragraph = new StringBuilder();
        StringBuilder code = null;
        for (String raw : lines) {
            String trimmed = raw.trim();
            if (code != null) {
                if (trimmed.startsWith("```")) {
                    out.add(new Block(BlockKind.CODE, 0, "", Collections.singletonList(new Span(SpanKind.CODE, code.toString(), null))));
                    code = null;
                } else {
                    if (code.length() > 0) code.append('\n');
                    code.append(raw);
                }
                continue;
            }
            if (trimmed.startsWith("```")) {
                flushParagraph(out, paragraph);
                code = new StringBuilder();
                continue;
            }
            if (trimmed.isEmpty()) {
                flushParagraph(out, paragraph);
                continue;
            }
            if (isRule(trimmed)) {
                flushParagraph(out, paragraph);
                out.add(new Block(BlockKind.RULE, 0, "", Collections.emptyList()));
                continue;
            }
            int heading = headingLevel(trimmed);
            if (heading > 0) {
                flushParagraph(out, paragraph);
                String text = trimmed.substring(heading).trim();
                while (text.endsWith("#") && text.length() > 1 && text.charAt(text.length() - 2) == ' ') {
                    text = text.substring(0, text.length() - 1).trim();
                }
                out.add(new Block(BlockKind.HEADING, heading, "", parseInline(text)));
                continue;
            }
            if (trimmed.startsWith(">")) {
                flushParagraph(out, paragraph);
                String quote = trimmed.substring(1).trim();
                out.add(new Block(BlockKind.QUOTE, 0, "", parseInline(quote)));
                continue;
            }
            String bullet = stripBullet(trimmed);
            if (bullet != null) {
                flushParagraph(out, paragraph);
                out.add(new Block(BlockKind.BULLET, 0, "• ", parseInline(bullet)));
                continue;
            }
            int orderedEnd = orderedPrefixEnd(trimmed);
            if (orderedEnd > 0) {
                flushParagraph(out, paragraph);
                String prefix = trimmed.substring(0, orderedEnd) + " ";
                String body = trimmed.substring(orderedEnd).trim();
                out.add(new Block(BlockKind.ORDERED, 0, prefix, parseInline(body)));
                continue;
            }
            if (paragraph.length() > 0) paragraph.append(' ');
            paragraph.append(trimmed);
        }
        if (code != null && code.length() > 0) {
            out.add(new Block(BlockKind.CODE, 0, "", Collections.singletonList(new Span(SpanKind.CODE, code.toString(), null))));
        }
        flushParagraph(out, paragraph);
        return out;
    }

    static Preview preview(List<Block> blocks, int maxChars) {
        if (maxChars <= 0) return new Preview(Collections.emptyList(), !blocks.isEmpty());
        List<Block> kept = new ArrayList<>();
        int used = 0;
        for (int i = 0; i < blocks.size(); i++) {
            Block b = blocks.get(i);
            String plain = b.plainText();
            int len = plain.codePointCount(0, plain.length());
            if (used + len <= maxChars) {
                kept.add(b);
                used += len + 1;
                continue;
            }
            int remaining = maxChars - used;
            if (remaining >= 24 && b.kind != BlockKind.RULE) {
                kept.add(truncateBlock(b, remaining));
            }
            return new Preview(kept, true);
        }
        return new Preview(kept, false);
    }

    private static Block truncateBlock(Block block, int maxChars) {
        List<Span> out = new ArrayList<>();
        int used = block.prefix.codePointCount(0, block.prefix.length());
        for (Span span : block.spans) {
            int len = span.text.codePointCount(0, span.text.length());
            if (used + len <= maxChars) {
                out.add(span);
                used += len;
                continue;
            }
            int take = Math.max(0, maxChars - used);
            if (take > 0) {
                String cut = span.text.substring(0, span.text.offsetByCodePoints(0, take)).trim();
                int space = cut.lastIndexOf(' ');
                if (space > cut.length() / 2) cut = cut.substring(0, space);
                if (!cut.isEmpty()) out.add(new Span(span.kind, cut + "…", span.url));
            }
            break;
        }
        if (out.isEmpty()) out.add(new Span(SpanKind.TEXT, "…", null));
        return new Block(block.kind, block.level, block.prefix, out);
    }

    private static void flushParagraph(List<Block> out, StringBuilder paragraph) {
        if (paragraph.length() == 0) return;
        out.add(new Block(BlockKind.PARAGRAPH, 0, "", parseInline(paragraph.toString())));
        paragraph.setLength(0);
    }

    private static boolean isRule(String line) {
        if (line.length() < 3) return false;
        char mark = line.charAt(0);
        if (mark != '-' && mark != '*' && mark != '_') return false;
        int count = 0;
        for (int i = 0; i < line.length(); i++) {
            char c = line.charAt(i);
            if (c == mark) count++;
            else if (c != ' ') return false;
        }
        return count >= 3;
    }

    private static int headingLevel(String line) {
        int n = 0;
        while (n < line.length() && n < 6 && line.charAt(n) == '#') n++;
        if (n == 0 || n >= line.length() || line.charAt(n) != ' ') return 0;
        return Math.min(n, 4);
    }

    private static String stripBullet(String line) {
        if (line.length() < 2) return null;
        char c = line.charAt(0);
        if ((c == '-' || c == '*' || c == '+') && line.charAt(1) == ' ') {
            return line.substring(2).trim();
        }
        return null;
    }

    private static int orderedPrefixEnd(String line) {
        int i = 0;
        while (i < line.length() && i < 4 && Character.isDigit(line.charAt(i))) i++;
        if (i == 0 || i + 1 >= line.length()) return 0;
        char sep = line.charAt(i);
        if ((sep == '.' || sep == ')') && line.charAt(i + 1) == ' ') return i + 1;
        return 0;
    }

    static List<Span> parseInline(String text) {
        List<Span> out = new ArrayList<>();
        if (text == null || text.isEmpty()) return out;
        StringBuilder plain = new StringBuilder();
        int i = 0, n = text.length();
        while (i < n) {
            char c = text.charAt(i);
            if (c == '\\' && i + 1 < n && isEscapable(text.charAt(i + 1))) {
                plain.append(text.charAt(i + 1));
                i += 2;
                continue;
            }
            if (c == '`') {
                int end = text.indexOf('`', i + 1);
                if (end > i + 1) {
                    flushPlain(out, plain);
                    out.add(new Span(SpanKind.CODE, text.substring(i + 1, end), null));
                    i = end + 1;
                    continue;
                }
            }
            if (c == '[') {
                int closeBracket = text.indexOf(']', i + 1);
                if (closeBracket > i + 1 && closeBracket + 1 < n && text.charAt(closeBracket + 1) == '(') {
                    int closeParen = text.indexOf(')', closeBracket + 2);
                    if (closeParen > closeBracket + 2) {
                        String label = text.substring(i + 1, closeBracket);
                        String url = text.substring(closeBracket + 2, closeParen).trim();
                        flushPlain(out, plain);
                        if (isSafeHttpsUrl(url)) {
                            out.add(new Span(SpanKind.LINK, label, url));
                        } else {
                            out.add(new Span(SpanKind.TEXT, label, null));
                        }
                        i = closeParen + 1;
                        continue;
                    }
                }
            }
            if (text.startsWith("***", i) || text.startsWith("___", i)) {
                String delim = text.substring(i, i + 3);
                int end = text.indexOf(delim, i + 3);
                if (end > i + 3) {
                    flushPlain(out, plain);
                    out.add(new Span(SpanKind.BOLD_ITALIC, text.substring(i + 3, end), null));
                    i = end + 3;
                    continue;
                }
            }
            if (text.startsWith("**", i) || text.startsWith("__", i)) {
                String delim = text.substring(i, i + 2);
                int end = text.indexOf(delim, i + 2);
                if (end > i + 2) {
                    flushPlain(out, plain);
                    out.add(new Span(SpanKind.BOLD, text.substring(i + 2, end), null));
                    i = end + 2;
                    continue;
                }
            }
            if (text.startsWith("~~", i)) {
                int end = text.indexOf("~~", i + 2);
                if (end > i + 2) {
                    flushPlain(out, plain);
                    out.add(new Span(SpanKind.STRIKE, text.substring(i + 2, end), null));
                    i = end + 2;
                    continue;
                }
            }
            if ((c == '*' || c == '_') && i + 1 < n && text.charAt(i + 1) != ' ' && text.charAt(i + 1) != c) {
                // Avoid turning snake_case identifiers into italics.
                boolean wordBefore = c == '_' && i > 0 && Character.isLetterOrDigit(text.charAt(i - 1));
                if (!wordBefore) {
                    int end = findClosingEmphasis(text, c, i + 1);
                    if (end > i + 1) {
                        flushPlain(out, plain);
                        out.add(new Span(SpanKind.ITALIC, text.substring(i + 1, end), null));
                        i = end + 1;
                        continue;
                    }
                }
            }
            plain.append(c);
            i++;
        }
        flushPlain(out, plain);
        return out;
    }

    private static int findClosingEmphasis(String text, char marker, int start) {
        int i = start;
        while (i < text.length()) {
            int at = text.indexOf(marker, i);
            if (at < 0) return -1;
            if (text.charAt(at - 1) != ' ') {
                boolean wordAfter = marker == '_' && at + 1 < text.length() && Character.isLetterOrDigit(text.charAt(at + 1));
                if (!wordAfter) return at;
            }
            i = at + 1;
        }
        return -1;
    }

    private static boolean isEscapable(char c) {
        return c == '\\' || c == '*' || c == '_' || c == '`' || c == '[' || c == ']' || c == '~' || c == '#';
    }

    private static void flushPlain(List<Span> out, StringBuilder plain) {
        if (plain.length() == 0) return;
        out.add(new Span(SpanKind.TEXT, plain.toString(), null));
        plain.setLength(0);
    }

    static boolean isSafeHttpsUrl(String url) {
        if (url == null || !url.regionMatches(true, 0, "https://", 0, 8)) return false;
        if (url.length() <= 8) return false;
        for (int i = 0; i < url.length(); i++) {
            char c = url.charAt(i);
            if (c <= 32 || c == '\\') return false;
        }
        int slash = url.indexOf('/', 8);
        String authority = slash < 0 ? url.substring(8) : url.substring(8, slash);
        return !authority.isEmpty() && authority.indexOf('@') < 0;
    }
}
