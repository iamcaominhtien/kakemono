using System;
using System.Collections.Generic;

namespace Kakemono
{
    /// <summary>An HTML mode, from the most faithful to the most readable (Markdown: Medium or Friendly).</summary>
    public enum Mode
    {
        /// <summary>A copy of every page, with a selectable text layer.</summary>
        Hard,

        /// <summary>Like Hard, but all text is real HTML text placed as in the PDF.</summary>
        Hard95,

        /// <summary>Page and column layout kept, text reflowed (Markdown: tables kept as HTML).</summary>
        Medium,

        /// <summary>A single-column article.</summary>
        Friendly,
    }

    /// <summary>How displayed formulas are written.</summary>
    public enum Equations
    {
        /// <summary>As MathML where their structure is clear.</summary>
        MathMl,

        /// <summary>As pictures.</summary>
        Image,
    }

    /// <summary>How the pages are drawn in the hard modes.</summary>
    public enum PageImage
    {
        /// <summary>SVG, with a PNG fallback for what SVG cannot draw.</summary>
        Svg,

        /// <summary>SVG only.</summary>
        SvgOnly,

        /// <summary>PNG pictures.</summary>
        Png,
    }

    /// <summary>How hard PNG pictures are compressed; the pixels are the same.</summary>
    public enum PngCompression
    {
        /// <summary>Several times faster, files about a quarter larger.</summary>
        Fast,

        /// <summary>The default.</summary>
        Balanced,

        /// <summary>About a tenth smaller, ten times slower.</summary>
        High,
    }

    /// <summary>The layout of the Typst output.</summary>
    public enum TypstLayout
    {
        /// <summary>A book when the document has chapters, an article otherwise.</summary>
        Auto,

        /// <summary>An article.</summary>
        Article,

        /// <summary>A printed book: title page, chapters on right-hand pages, running heads.</summary>
        Book,
    }

    /// <summary>The names kakemono gives the values of the enums.</summary>
    internal static class Names
    {
        internal static string Of(Mode value) => value switch
        {
                Mode.Hard => "hard",
                Mode.Hard95 => "hard-95",
                Mode.Medium => "medium",
                Mode.Friendly => "friendly",
            _ => throw new ArgumentOutOfRangeException(nameof(value)),
        };

        internal static string Of(Equations value) => value switch
        {
                Equations.MathMl => "mathml",
                Equations.Image => "image",
            _ => throw new ArgumentOutOfRangeException(nameof(value)),
        };

        internal static string Of(PageImage value) => value switch
        {
                PageImage.Svg => "svg",
                PageImage.SvgOnly => "svg-only",
                PageImage.Png => "png",
            _ => throw new ArgumentOutOfRangeException(nameof(value)),
        };

        internal static string Of(PngCompression value) => value switch
        {
                PngCompression.Fast => "fast",
                PngCompression.Balanced => "balanced",
                PngCompression.High => "high",
            _ => throw new ArgumentOutOfRangeException(nameof(value)),
        };

        internal static string Of(TypstLayout value) => value switch
        {
                TypstLayout.Auto => "auto",
                TypstLayout.Article => "article",
                TypstLayout.Book => "book",
            _ => throw new ArgumentOutOfRangeException(nameof(value)),
        };

    }
}
