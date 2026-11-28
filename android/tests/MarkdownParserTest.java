package org.nvforever.android;

import java.util.List;

/** Pure-Java unit tests for MarkdownParser (headings, lists, inline styles, links, preview). */
public final class MarkdownParserTest {
    private static int checks = 0;

    private static void check(boolean condition, String detail) {
        checks++;
        if (!condition) throw new AssertionError("FAILED: " + detail);
    }

    public static void main(String[] args) {
        String sample = ""
            + "# Release 0.8.1\n"
            + "\n"
            + "## Highlights\n"
            + "- **Bold** and *italic* and `inline_code` with ~~old~~ text\n"
            + "- Link to [NVForever](https://nv-forever.com/news) and unsafe [bad](javascript:alert(1))\n"
            + "1. First step\n"
            + "2. Second step\n"
            + "\n"
            + "> Quoted note with snake_case_ident intact\n"
            + "\n"
            + "---\n"
            + "\n"
            + "```bash\n"
            + "cargo test --locked\n"
            + "```\n"
            + "\n"
            + "Wrapped paragraph line one\n"
            + "and line two.\n";

        List<MarkdownParser.Block> blocks = MarkdownParser.parse(sample);
        check(blocks.size() == 10, "expected 10 blocks, got " + blocks.size());
        check(blocks.get(0).kind == MarkdownParser.BlockKind.HEADING && blocks.get(0).level == 1, "H1");
        check("Release 0.8.1".equals(blocks.get(0).plainText()), "H1 text");
        check(blocks.get(1).kind == MarkdownParser.BlockKind.HEADING && blocks.get(1).level == 2, "H2");
        check(blocks.get(2).kind == MarkdownParser.BlockKind.BULLET, "bullet 1");
        check(blocks.get(2).spans.stream().anyMatch(s -> s.kind == MarkdownParser.SpanKind.BOLD && "Bold".equals(s.text)), "bold span");
        check(blocks.get(2).spans.stream().anyMatch(s -> s.kind == MarkdownParser.SpanKind.ITALIC && "italic".equals(s.text)), "italic span");
        check(blocks.get(2).spans.stream().anyMatch(s -> s.kind == MarkdownParser.SpanKind.CODE && "inline_code".equals(s.text)), "code span");
        check(blocks.get(2).spans.stream().anyMatch(s -> s.kind == MarkdownParser.SpanKind.STRIKE && "old".equals(s.text)), "strike span");

        check(blocks.get(3).spans.stream().anyMatch(s -> s.kind == MarkdownParser.SpanKind.LINK
            && "NVForever".equals(s.text) && "https://nv-forever.com/news".equals(s.url)), "safe https link");
        check(blocks.get(3).spans.stream().noneMatch(s -> s.kind == MarkdownParser.SpanKind.LINK && "bad".equals(s.text)), "unsafe scheme stripped");

        check(blocks.get(4).kind == MarkdownParser.BlockKind.ORDERED && "1. ".equals(blocks.get(4).prefix), "ordered 1");
        check(blocks.get(5).kind == MarkdownParser.BlockKind.ORDERED && "2. ".equals(blocks.get(5).prefix), "ordered 2");

        check(blocks.get(6).kind == MarkdownParser.BlockKind.QUOTE, "quote block");
        check(blocks.get(6).plainText().contains("snake_case_ident"), "snake_case preserved");
        check(blocks.get(6).spans.stream().noneMatch(s -> s.kind == MarkdownParser.SpanKind.ITALIC), "snake_case not italic");

        check(blocks.get(7).kind == MarkdownParser.BlockKind.RULE, "horizontal rule");
        check(blocks.get(8).kind == MarkdownParser.BlockKind.CODE && "cargo test --locked".equals(blocks.get(8).plainText()), "fenced code");
        check(blocks.get(9).kind == MarkdownParser.BlockKind.PARAGRAPH && "Wrapped paragraph line one and line two.".equals(blocks.get(9).plainText()), "joined paragraph");

        // Paragraph line joining
        List<MarkdownParser.Block> para = MarkdownParser.parse("First line\nsecond line.\n");
        check(para.size() == 1 && "First line second line.".equals(para.get(0).plainText()), "paragraph join");

        // Preview truncation without raw syntax leakage
        MarkdownParser.Preview shortPreview = MarkdownParser.preview(blocks, 60);
        check(shortPreview.truncated, "preview truncated");
        check(!shortPreview.blocks.isEmpty(), "preview non-empty");
        MarkdownParser.Preview fullPreview = MarkdownParser.preview(blocks, 5000);
        check(!fullPreview.truncated && fullPreview.blocks.size() == blocks.size(), "full preview");

        // Link safety
        check(MarkdownParser.isSafeHttpsUrl("https://github.com/LootJunkieAzza/NVForever"), "valid https");
        check(!MarkdownParser.isSafeHttpsUrl("http://github.com/foo"), "reject http");
        check(!MarkdownParser.isSafeHttpsUrl("https://user@github.com/foo"), "reject userinfo");
        check(!MarkdownParser.isSafeHttpsUrl("https://github.com/a b"), "reject whitespace");

        System.out.println("MarkdownParserTest: PASS (" + checks + " checks)");
    }
}
