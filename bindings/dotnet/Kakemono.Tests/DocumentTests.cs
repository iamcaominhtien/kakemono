using System;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using Xunit;

namespace Kakemono.Tests
{
    /// <summary>The .NET API on the repository's samples (tests/data).</summary>
    public class DocumentTests
    {
        private static readonly string Data = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../../../tests/data"));
        private static readonly string Ligo = Path.Combine(Data, "1602.03837v1.pdf");
        private static readonly string Docx = Path.Combine(Data, "word/sample.docx");

        [Fact]
        public void APdfIsWrittenAsMarkdownWithItsPictures()
        {
            using var doc = Document.Open(Ligo, new OpenOptions { Jobs = 1 });
            Assert.Equal("pdf", doc.Format);
            Assert.True(doc.PageCount > 10);
            Assert.Contains("Gravitational Waves", doc.Title);
            TextOutput md = doc.ToMarkdown();
            Assert.Contains("Observation of Gravitational Waves", md.Text);
            Assert.NotEmpty(md.Files);
            Assert.All(md.Files, file => Assert.StartsWith("images/", file.Path));
            string dir = Path.Combine(Path.GetTempPath(), "kakemono-dotnet-" + Guid.NewGuid());
            md.WriteTo(Path.Combine(dir, "paper.md"));
            Assert.True(File.Exists(Path.Combine(dir, md.Files[0].Path)));
            Directory.Delete(dir, recursive: true);
            Assert.Empty(doc.ToMarkdown(new MarkdownOptions { InlineImages = true }).Files);
            Assert.StartsWith("<!DOCTYPE html>", doc.ToHtml(new HtmlOptions { Mode = Mode.Hard95 }).Text);
            Assert.Contains("\"blocks\"", doc.Analyze());
        }

        [Fact]
        public void ChunksHaveTheirSectionAndPages()
        {
            using var doc = Document.Open(Ligo, new OpenOptions { Jobs = 1 });
            var chunks = doc.ToChunks(new ChunkOptions { MaxTokens = 256 });
            Assert.True(chunks.Count > 10);
            Assert.Equal(0, chunks[0].Id);
            Assert.Contains(chunks, chunk => chunk.Section.Count > 0);
            Assert.Contains(chunks, chunk => chunk.Pages.Count > 0);
        }

        [Fact]
        public void AWordDocumentFromBytesGivesBinaryOutputs()
        {
            using var doc = Document.Open(File.ReadAllBytes(Docx));
            Assert.Equal("docx", doc.Format);
            Assert.Equal(0, doc.PageCount);
            Assert.Equal((byte)'P', doc.ToDocx().Data[0]);
            Assert.Equal("%PDF", System.Text.Encoding.ASCII.GetString(doc.ToPdf().Data, 0, 4));
            Assert.Equal((byte)'P', doc.ToEpub(new EpubOptions { Modified = "2026-01-31T12:00:00Z" }).Data[0]);
        }

        [Fact]
        public void EditedContentShowsInEveryOutput()
        {
            string json;
            using (var doc = Document.Open(Docx))
            {
                json = doc.ToJson().Text;
            }
            Assert.Contains("Convergence of Gradient Descent", json);
            string edited = json.Replace("Convergence of Gradient Descent", "Đã dịch");
            using var translated = Document.Open(Docx, new OpenOptions { Content = edited });
            Assert.Empty(translated.Warnings);
            Assert.Contains("Đã dịch", translated.ToMarkdown().Text);
            var err = Assert.Throws<KakemonoException>(() => translated.ToHtml(new HtmlOptions { Mode = Mode.Medium }));
            Assert.Equal(KakemonoErrorKind.InvalidArgument, err.Kind);
        }

        [Fact]
        public void ErrorsHaveAKindAndAMessage()
        {
            var unreadable = Assert.Throws<KakemonoException>(() => Document.Open(new byte[] { 1, 2, 3 }));
            Assert.Equal(KakemonoErrorKind.Document, unreadable.Kind);
            using var doc = Document.Open(Docx);
            var scale = Assert.Throws<KakemonoException>(() => doc.ToMarkdown(new MarkdownOptions { Scale = 9 }));
            Assert.Equal(KakemonoErrorKind.InvalidArgument, scale.Kind);
            Assert.Contains("scale must be between", scale.Message);
            Assert.Throws<KakemonoException>(() => doc.ToMarkdown(new MarkdownOptions { ImageDir = "../up" }));
            var missing = Assert.Throws<KakemonoException>(() => Document.Open("/no/such.pdf"));
            Assert.Contains("/no/such.pdf", missing.Message);
            Assert.Throws<KakemonoException>(() => Document.AddFont(new byte[] { 1, 2, 3 }));
            Assert.Matches(@"^\d+\.\d+\.\d+", Document.Version);
        }

        [Fact]
        public void ADisposedDocumentRefusesCalls()
        {
            var doc = Document.Open(Docx);
            doc.Dispose();
            doc.Dispose();
            Assert.Throws<ObjectDisposedException>(() => doc.ToMarkdown());
            Assert.Throws<ObjectDisposedException>(() => doc.PageCount);
        }

        [Fact]
        public async Task OutputsRunFromSeveralThreadsAndDisposeWaitsForThem()
        {
            var doc = await Document.OpenAsync(Ligo, new OpenOptions { Jobs = 1 });
            var tasks = Enumerable.Range(0, 4).Select(_ => doc.ToMarkdownAsync()).ToList();
            var chunks = doc.ToChunksAsync();
            int done = 0;
            foreach (var task in tasks)
            {
                Assert.True((await task).Text.Length > 1000);
                done++;
            }
            Assert.True((await chunks).Count > 10);
            // Disposing while a render runs frees the document after it.
            var running = doc.ToHtmlAsync();
            doc.Dispose();
            try
            {
                Assert.True((await running).Text.Length > 1000);
            }
            catch (ObjectDisposedException)
            {
                // It had not started yet.
            }
            Assert.Equal(4, done);
        }
    }
}
