using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Text.Json.Serialization;

namespace Kakemono
{
    /// <summary>A picture file of an output.</summary>
    public sealed class OutputFile
    {
        internal OutputFile(string path, byte[] data)
        {
            Path = path;
            Data = data;
        }

        /// <summary>Its path, relative to the folder of the output (<c>images/fig-1.png</c>).</summary>
        public string Path { get; }

        /// <summary>Its bytes.</summary>
        public byte[] Data { get; }

        /// <summary>Writes the file under <paramref name="folder"/>, making its folders.</summary>
        public void WriteTo(string folder)
        {
            string root = System.IO.Path.GetFullPath(folder);
            string target = System.IO.Path.GetFullPath(System.IO.Path.Combine(root, Path));
            if (!target.StartsWith(root.TrimEnd(System.IO.Path.DirectorySeparatorChar) + System.IO.Path.DirectorySeparatorChar, StringComparison.Ordinal))
            {
                throw new IOException($"the picture path {Path} leaves {folder}");
            }
            Directory.CreateDirectory(System.IO.Path.GetDirectoryName(target)!);
            File.WriteAllBytes(target, Data);
        }

        internal static void WriteAll(IReadOnlyList<OutputFile> files, string file)
        {
            string folder = System.IO.Path.GetDirectoryName(System.IO.Path.GetFullPath(file))!;
            Directory.CreateDirectory(folder);
            foreach (OutputFile picture in files)
            {
                picture.WriteTo(folder);
            }
        }
    }

    /// <summary>A text output (HTML, Markdown, JSON, LaTeX, Typst) and its picture files.</summary>
    public sealed class TextOutput
    {
        internal TextOutput(string text, IReadOnlyList<OutputFile> files)
        {
            Text = text;
            Files = files;
        }

        /// <summary>The text.</summary>
        public string Text { get; }

        /// <summary>The picture files to write next to it (empty when the pictures are inline).</summary>
        public IReadOnlyList<OutputFile> Files { get; }

        /// <summary>Writes the text to <paramref name="file"/> (UTF-8) and the pictures next to it.</summary>
        public void WriteTo(string file)
        {
            OutputFile.WriteAll(Files, file);
            File.WriteAllText(file, Text, new UTF8Encoding(false));
        }
    }

    /// <summary>A binary output (Word document, EPUB book, PDF) and its picture files.</summary>
    public sealed class BinaryOutput
    {
        internal BinaryOutput(byte[] data, IReadOnlyList<OutputFile> files)
        {
            Data = data;
            Files = files;
        }

        /// <summary>The bytes.</summary>
        public byte[] Data { get; }

        /// <summary>The picture files to write next to it (a Word document's, with ImageDir).</summary>
        public IReadOnlyList<OutputFile> Files { get; }

        /// <summary>Writes the bytes to <paramref name="file"/> and the pictures next to it.</summary>
        public void WriteTo(string file)
        {
            OutputFile.WriteAll(Files, file);
            File.WriteAllBytes(file, Data);
        }
    }

    /// <summary>A chunk for retrieval (RAG), cut at sentence ends.</summary>
    public sealed class Chunk
    {
        /// <summary>Number of the chunk, in reading order, from 0.</summary>
        [JsonPropertyName("id")]
        public int Id { get; set; }

        /// <summary>The text to embed: the section path, a blank line, then the text.</summary>
        [JsonPropertyName("text")]
        public string Text { get; set; } = "";

        /// <summary>Its size in tokens.</summary>
        [JsonPropertyName("tokens")]
        public int Tokens { get; set; }

        /// <summary>The title of the document.</summary>
        [JsonPropertyName("title")]
        public string? Title { get; set; }

        /// <summary>The headings of the section it is in, from the outermost.</summary>
        [JsonPropertyName("section")]
        public IReadOnlyList<string> Section { get; set; } = Array.Empty<string>();

        /// <summary>The blocks of the document it holds (their numbers).</summary>
        [JsonPropertyName("blocks")]
        public IReadOnlyList<int> Blocks { get; set; } = Array.Empty<int>();

        /// <summary>The pages it comes from, from 0 (<c>[0]</c> for documents without pages).</summary>
        [JsonPropertyName("pages")]
        public IReadOnlyList<int> Pages { get; set; } = Array.Empty<int>();
    }
}
