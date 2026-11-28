using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

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
        public int Id { get; private set; }

        /// <summary>The text to embed: the section path, a blank line, then the text.</summary>
        public string Text { get; private set; } = "";

        /// <summary>Its size in tokens.</summary>
        public int Tokens { get; private set; }

        /// <summary>The title of the document.</summary>
        public string? Title { get; private set; }

        /// <summary>The headings of the section it is in, from the outermost.</summary>
        public IReadOnlyList<string> Section { get; private set; } = Array.Empty<string>();

        /// <summary>The blocks of the document it holds (their numbers).</summary>
        public IReadOnlyList<int> Blocks { get; private set; } = Array.Empty<int>();

        /// <summary>The pages it comes from, from 0 (<c>[0]</c> for documents without pages).</summary>
        public IReadOnlyList<int> Pages { get; private set; } = Array.Empty<int>();

        /// <summary>A chunk from one line of kakemono's JSON Lines (no reflection).</summary>
        internal static Chunk Read(string line)
        {
            using var json = System.Text.Json.JsonDocument.Parse(line);
            var root = json.RootElement;
            var chunk = new Chunk
            {
                Id = root.GetProperty("id").GetInt32(),
                Text = root.GetProperty("text").GetString() ?? "",
                Tokens = root.GetProperty("tokens").GetInt32(),
                Section = Strings(root, "section"),
                Blocks = Numbers(root, "blocks"),
                Pages = Numbers(root, "pages"),
            };
            if (root.TryGetProperty("title", out var title) && title.ValueKind == System.Text.Json.JsonValueKind.String)
            {
                chunk.Title = title.GetString();
            }
            return chunk;
        }

        private static IReadOnlyList<string> Strings(System.Text.Json.JsonElement root, string name)
        {
            var items = new List<string>();
            if (root.TryGetProperty(name, out var array))
            {
                foreach (var item in array.EnumerateArray())
                {
                    items.Add(item.GetString() ?? "");
                }
            }
            return items;
        }

        private static IReadOnlyList<int> Numbers(System.Text.Json.JsonElement root, string name)
        {
            var items = new List<int>();
            if (root.TryGetProperty(name, out var array))
            {
                foreach (var item in array.EnumerateArray())
                {
                    items.Add(item.GetInt32());
                }
            }
            return items;
        }
    }
}
