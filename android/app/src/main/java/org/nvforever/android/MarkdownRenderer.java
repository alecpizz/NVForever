package org.nvforever.android;

import android.graphics.Typeface;
import android.text.SpannableStringBuilder;
import android.text.Spanned;
import android.text.TextPaint;
import android.text.method.LinkMovementMethod;
import android.text.style.ClickableSpan;
import android.text.style.ForegroundColorSpan;
import android.text.style.LeadingMarginSpan;
import android.text.style.RelativeSizeSpan;
import android.text.style.StrikethroughSpan;
import android.text.style.StyleSpan;
import android.text.style.TypefaceSpan;
import android.view.View;
import android.widget.TextView;
import java.util.List;
import java.util.function.Consumer;

/** Renders parsed Markdown blocks into Android Spannable text for launcher cards. */
final class MarkdownRenderer {
    private MarkdownRenderer() {}

    static void apply(TextView view, List<MarkdownParser.Block> blocks, int gold, int weak, int indentPx, Consumer<String> onLink) {
        SpannableStringBuilder out = new SpannableStringBuilder();
        boolean hasLinks = false;
        for (int i = 0; i < blocks.size(); i++) {
            if (i > 0) out.append("\n\n");
            MarkdownParser.Block block = blocks.get(i);
            int blockStart = out.length();
            if (block.kind == MarkdownParser.BlockKind.RULE) {
                out.append("────────────");
                out.setSpan(new ForegroundColorSpan(weak), blockStart, out.length(), Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                continue;
            }
            if (!block.prefix.isEmpty()) {
                int pStart = out.length();
                out.append(block.prefix);
                out.setSpan(new ForegroundColorSpan(gold), pStart, out.length(), Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                out.setSpan(new StyleSpan(Typeface.BOLD), pStart, out.length(), Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
            }
            for (MarkdownParser.Span span : block.spans) {
                int start = out.length();
                out.append(span.text);
                int end = out.length();
                switch (span.kind) {
                    case BOLD -> out.setSpan(new StyleSpan(Typeface.BOLD), start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    case ITALIC -> out.setSpan(new StyleSpan(Typeface.ITALIC), start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    case BOLD_ITALIC -> out.setSpan(new StyleSpan(Typeface.BOLD_ITALIC), start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    case CODE -> {
                        out.setSpan(new TypefaceSpan("monospace"), start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                        out.setSpan(new RelativeSizeSpan(0.92f), start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                        out.setSpan(new ForegroundColorSpan(block.kind == MarkdownParser.BlockKind.CODE ? weak : gold), start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    }
                    case STRIKE -> out.setSpan(new StrikethroughSpan(), start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    case LINK -> {
                        if (span.url != null && onLink != null) {
                            hasLinks = true;
                            final String target = span.url;
                            out.setSpan(new ClickableSpan() {
                                @Override public void onClick(View widget) { onLink.accept(target); }
                                @Override public void updateDrawState(TextPaint ds) {
                                    ds.setColor(gold);
                                    ds.setUnderlineText(true);
                                }
                            }, start, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                        }
                    }
                    case TEXT -> {}
                }
            }
            int blockEnd = out.length();
            switch (block.kind) {
                case HEADING -> {
                    float scale = switch (block.level) {
                        case 1 -> 1.28f;
                        case 2 -> 1.18f;
                        case 3 -> 1.10f;
                        default -> 1.04f;
                    };
                    out.setSpan(new StyleSpan(Typeface.BOLD), blockStart, blockEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    out.setSpan(new RelativeSizeSpan(scale), blockStart, blockEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    out.setSpan(new ForegroundColorSpan(gold), blockStart, blockEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                }
                case BULLET, ORDERED -> out.setSpan(new LeadingMarginSpan.Standard(indentPx / 2, indentPx), blockStart, blockEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                case QUOTE -> {
                    out.setSpan(new LeadingMarginSpan.Standard(indentPx), blockStart, blockEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    out.setSpan(new StyleSpan(Typeface.ITALIC), blockStart, blockEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    out.setSpan(new ForegroundColorSpan(weak), blockStart, blockEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                }
                case CODE -> out.setSpan(new LeadingMarginSpan.Standard(indentPx / 2), blockStart, blockEnd, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                case PARAGRAPH, RULE -> {}
            }
        }
        view.setText(out);
        if (hasLinks) {
            view.setMovementMethod(LinkMovementMethod.getInstance());
            view.setLinksClickable(true);
        }
    }
}
