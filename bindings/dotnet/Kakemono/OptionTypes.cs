using System.Collections.Generic;

namespace Kakemono
{
    /// <summary>Options of reading and analysing a document, kept for every output.</summary>
    public sealed class OpenOptions : Options
    {
        /// <summary>The name recorded as the document's source (by default: the file name, the address, <c>arXiv:&lt;id&gt;</c>, nothing for bytes).</summary>
        public string? SourceName { get; set; }

        /// <summary>Rebuild tables as real tables when their grid is clear (default true).</summary>
        public bool? Tables { get; set; }

        /// <summary>Worker threads for one document, 0 for one per core (default 0). A server converting several documents at once should use 1 or 2.</summary>
        public int? Jobs { get; set; }

        /// <summary>How hard PNG pictures are compressed (default Balanced).</summary>
        public PngCompression? PngCompression { get; set; }

        /// <summary>An edited copy of the JSON of <see cref="Document.ToJson"/> for the same document (its text): every output shows its text.</summary>
        public string? Content { get; set; }

        /// <summary>The folder a relative <c>replace_image</c> of the content is read from.</summary>
        public string? ContentDir { get; set; }

        /// <summary>The header rows of every table of an Excel workbook, given instead of found.</summary>
        public int? HeaderRows { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (SourceName is { } sourcenameValue) values["sourceName"] = sourcenameValue;
            if (Tables is { } tablesValue) values["tables"] = tablesValue;
            if (Jobs is { } jobsValue) values["jobs"] = jobsValue;
            if (PngCompression is { } pngcompressionValue) values["pngCompression"] = Names.Of(pngcompressionValue);
            if (Content is { } contentValue) values["content"] = contentValue;
            if (ContentDir is { } contentdirValue) values["contentDir"] = contentdirValue;
            if (HeaderRows is { } headerrowsValue) values["headerRows"] = headerrowsValue;
        }
    }

    /// <summary>Options of the HTML output (pictures inline unless InlineImages is false).</summary>
    public sealed class HtmlOptions : Options
    {
        /// <summary>The mode (default Friendly).</summary>
        public Mode? Mode { get; set; }

        /// <summary>How displayed formulas are written (default MathMl).</summary>
        public Equations? Equations { get; set; }

        /// <summary>Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3).</summary>
        public double? Scale { get; set; }

        /// <summary>How the pages are drawn in the hard modes (default Svg).</summary>
        public PageImage? PageImage { get; set; }

        /// <summary>Embed the fonts of the PDF (default true).</summary>
        public bool? EmbedFonts { get; set; }

        /// <summary>Pictures as data URIs instead of files.</summary>
        public bool? InlineImages { get; set; }

        /// <summary>The folder of the picture files, relative to the output's (default <c>images</c>).</summary>
        public string? ImageDir { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (Mode is { } modeValue) values["mode"] = Names.Of(modeValue);
            if (Equations is { } equationsValue) values["equations"] = Names.Of(equationsValue);
            if (Scale is { } scaleValue) values["scale"] = scaleValue;
            if (PageImage is { } pageimageValue) values["pageImage"] = Names.Of(pageimageValue);
            if (EmbedFonts is { } embedfontsValue) values["embedFonts"] = embedfontsValue;
            if (InlineImages is { } inlineimagesValue) values["inlineImages"] = inlineimagesValue;
            if (ImageDir is { } imagedirValue) values["imageDir"] = imagedirValue;
        }
    }

    /// <summary>Options of the Markdown output.</summary>
    public sealed class MarkdownOptions : Options
    {
        /// <summary>Medium keeps tables as HTML; the default is Friendly (the hard modes do not apply).</summary>
        public Mode? Mode { get; set; }

        /// <summary>How displayed formulas are written (default MathMl).</summary>
        public Equations? Equations { get; set; }

        /// <summary>Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3).</summary>
        public double? Scale { get; set; }

        /// <summary>The folder of the picture files, relative to the output's (default <c>images</c>).</summary>
        public string? ImageDir { get; set; }

        /// <summary>Pictures as data URIs instead of files.</summary>
        public bool? InlineImages { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (Mode is { } modeValue) values["mode"] = Names.Of(modeValue);
            if (Equations is { } equationsValue) values["equations"] = Names.Of(equationsValue);
            if (Scale is { } scaleValue) values["scale"] = scaleValue;
            if (ImageDir is { } imagedirValue) values["imageDir"] = imagedirValue;
            if (InlineImages is { } inlineimagesValue) values["inlineImages"] = inlineimagesValue;
        }
    }

    /// <summary>Options of the editable JSON output.</summary>
    public sealed class JsonOptions : Options
    {
        /// <summary>How displayed formulas are written (default MathMl).</summary>
        public Equations? Equations { get; set; }

        /// <summary>Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3).</summary>
        public double? Scale { get; set; }

        /// <summary>The folder of the picture files, relative to the output's (default <c>images</c>).</summary>
        public string? ImageDir { get; set; }

        /// <summary>Pictures as data URIs instead of files.</summary>
        public bool? InlineImages { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (Equations is { } equationsValue) values["equations"] = Names.Of(equationsValue);
            if (Scale is { } scaleValue) values["scale"] = scaleValue;
            if (ImageDir is { } imagedirValue) values["imageDir"] = imagedirValue;
            if (InlineImages is { } inlineimagesValue) values["inlineImages"] = inlineimagesValue;
        }
    }

    /// <summary>Options of the LaTeX output.</summary>
    public sealed class LatexOptions : Options
    {
        /// <summary>How displayed formulas are written (default MathMl).</summary>
        public Equations? Equations { get; set; }

        /// <summary>Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3).</summary>
        public double? Scale { get; set; }

        /// <summary>The folder of the picture files, relative to the output's (default <c>images</c>).</summary>
        public string? ImageDir { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (Equations is { } equationsValue) values["equations"] = Names.Of(equationsValue);
            if (Scale is { } scaleValue) values["scale"] = scaleValue;
            if (ImageDir is { } imagedirValue) values["imageDir"] = imagedirValue;
        }
    }

    /// <summary>Options of the Typst output.</summary>
    public sealed class TypstOptions : Options
    {
        /// <summary>How displayed formulas are written (default MathMl).</summary>
        public Equations? Equations { get; set; }

        /// <summary>Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3).</summary>
        public double? Scale { get; set; }

        /// <summary>The folder of the picture files, relative to the output's (default <c>images</c>).</summary>
        public string? ImageDir { get; set; }

        /// <summary>The layout (default Auto).</summary>
        public TypstLayout? Layout { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (Equations is { } equationsValue) values["equations"] = Names.Of(equationsValue);
            if (Scale is { } scaleValue) values["scale"] = scaleValue;
            if (ImageDir is { } imagedirValue) values["imageDir"] = imagedirValue;
            if (Layout is { } layoutValue) values["layout"] = Names.Of(layoutValue);
        }
    }

    /// <summary>Options of the Word output: with ImageDir, its pictures are files too.</summary>
    public sealed class DocxOptions : Options
    {
        /// <summary>How displayed formulas are written (default MathMl).</summary>
        public Equations? Equations { get; set; }

        /// <summary>Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3).</summary>
        public double? Scale { get; set; }

        /// <summary>The folder of the picture files, relative to the output's (default <c>images</c>).</summary>
        public string? ImageDir { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (Equations is { } equationsValue) values["equations"] = Names.Of(equationsValue);
            if (Scale is { } scaleValue) values["scale"] = scaleValue;
            if (ImageDir is { } imagedirValue) values["imageDir"] = imagedirValue;
        }
    }

    /// <summary>Options of the EPUB output.</summary>
    public sealed class EpubOptions : Options
    {
        /// <summary>How displayed formulas are written (default MathMl).</summary>
        public Equations? Equations { get; set; }

        /// <summary>Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3).</summary>
        public double? Scale { get; set; }

        /// <summary>Widest formula kept as MathML, in em (default 20).</summary>
        public double? MaxMathWidthEm { get; set; }

        /// <summary>A cover page (default true).</summary>
        public bool? Cover { get; set; }

        /// <summary>The book's date, <c>CCYY-MM-DDThh:mm:ssZ</c> (default: a fixed date).</summary>
        public string? Modified { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (Equations is { } equationsValue) values["equations"] = Names.Of(equationsValue);
            if (Scale is { } scaleValue) values["scale"] = scaleValue;
            if (MaxMathWidthEm is { } maxmathwidthemValue) values["maxMathWidthEm"] = maxmathwidthemValue;
            if (Cover is { } coverValue) values["cover"] = coverValue;
            if (Modified is { } modifiedValue) values["modified"] = modifiedValue;
        }
    }

    /// <summary>Options of the chunks for retrieval.</summary>
    public sealed class ChunkOptions : Options
    {
        /// <summary>Largest chunk, in tokens (default 512).</summary>
        public int? MaxTokens { get; set; }

        /// <summary>The path of a Hugging Face <c>tokenizer.json</c>, or <c>cl100k_base</c> / <c>o200k_base</c>; tokens are estimated without it.</summary>
        public string? Tokenizer { get; set; }

        internal override void Fill(Dictionary<string, object> values)
        {
            if (MaxTokens is { } maxtokensValue) values["maxTokens"] = maxtokensValue;
            if (Tokenizer is { } tokenizerValue) values["tokenizer"] = tokenizerValue;
        }
    }

}
