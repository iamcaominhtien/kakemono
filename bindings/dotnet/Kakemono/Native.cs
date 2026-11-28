using System;
using System.Runtime.InteropServices;

namespace Kakemono
{
    /// <summary>The C ABI of kakemono (bindings/c/include/kakemono.h).</summary>
    internal static class Native
    {
        private const string Library = "kakemono_ffi";

        /// <summary>The version of the C ABI this binding calls.</summary>
        internal const uint AbiVersion = 1;

        /// <summary>Error kind of a call stopped by its <c>KkCancel</c>.</summary>
        internal const uint ErrorCancelled = 4;

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern uint kk_abi_version();

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_version(out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern UIntPtr kk_add_font(byte[] data, UIntPtr len, out IntPtr err);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern DocumentHandle kk_open_data(byte[] data, UIntPtr len, byte[]? options, IntPtr cancel, out IntPtr err);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern DocumentHandle kk_open_path(byte[] path, byte[]? options, IntPtr cancel, out IntPtr err);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern DocumentHandle kk_open_url(byte[] url, byte[]? options, IntPtr cancel, out IntPtr err);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern DocumentHandle kk_open_arxiv(byte[] id, byte[]? options, IntPtr cancel, out IntPtr err);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_document_format(DocumentHandle doc, out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_document_title(DocumentHandle doc, out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern UIntPtr kk_document_page_count(DocumentHandle doc);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern UIntPtr kk_document_warning_count(DocumentHandle doc);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_document_warning(DocumentHandle doc, UIntPtr index, out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_render(DocumentHandle doc, byte[] target, byte[]? options, IntPtr cancel, out IntPtr err);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern CancelHandle kk_cancel_new(double timeoutMs, out IntPtr err);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void kk_cancel_cancel(IntPtr cancel);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void kk_cancel_free(IntPtr cancel);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_output_text(IntPtr output, out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_output_data(IntPtr output, out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern UIntPtr kk_output_file_count(IntPtr output);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_output_file_path(IntPtr output, UIntPtr index, out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_output_file_data(IntPtr output, UIntPtr index, out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern uint kk_error_kind(IntPtr err);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr kk_error_message(IntPtr err, out UIntPtr len);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void kk_document_free(IntPtr doc);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void kk_output_free(IntPtr output);

        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void kk_error_free(IntPtr err);

        /// <summary>Text as the C ABI reads it: UTF-8, then a NUL.</summary>
        internal static byte[] Utf8(string text)
        {
            byte[] bytes = new byte[System.Text.Encoding.UTF8.GetByteCount(text) + 1];
            System.Text.Encoding.UTF8.GetBytes(text, 0, text.Length, bytes, 0);
            return bytes;
        }

        /// <summary>The <paramref name="len"/> bytes at <paramref name="ptr"/>.</summary>
        internal static byte[] Bytes(IntPtr ptr, UIntPtr len)
        {
            int count = checked((int)len.ToUInt64());
            byte[] bytes = new byte[count];
            if (count > 0)
            {
                Marshal.Copy(ptr, bytes, 0, count);
            }
            return bytes;
        }

        /// <summary>The UTF-8 text of (<paramref name="ptr"/>, <paramref name="len"/>), or null.</summary>
        internal static string? Text(IntPtr ptr, UIntPtr len) =>
            ptr == IntPtr.Zero ? null : System.Text.Encoding.UTF8.GetString(Bytes(ptr, len));

        /// <summary>Throws the error <paramref name="err"/> (and frees it) when it is not null.</summary>
        internal static void Check(IntPtr err)
        {
            if (err == IntPtr.Zero)
            {
                return;
            }
            try
            {
                IntPtr text = kk_error_message(err, out UIntPtr len);
                KakemonoErrorKind kind = kk_error_kind(err) switch
                {
                    1 => KakemonoErrorKind.InvalidArgument,
                    2 => KakemonoErrorKind.Document,
                    4 => KakemonoErrorKind.Cancelled,
                    5 => KakemonoErrorKind.TimedOut,
                    _ => KakemonoErrorKind.Internal,
                };
                throw new KakemonoException(Text(text, len) ?? "unknown error", kind);
            }
            finally
            {
                kk_error_free(err);
            }
        }
    }

    /// <summary>A request to stop calls of the C ABI (a <c>KkCancel</c>).</summary>
    internal sealed class CancelHandle : SafeHandle
    {
        public CancelHandle()
            : base(IntPtr.Zero, ownsHandle: true)
        {
        }

        public override bool IsInvalid => handle == IntPtr.Zero;

        protected override bool ReleaseHandle()
        {
            Native.kk_cancel_free(handle);
            return true;
        }
    }

    /// <summary>A document of the C ABI, freed once no call uses it.</summary>
    internal sealed class DocumentHandle : SafeHandle
    {
        public DocumentHandle()
            : base(IntPtr.Zero, ownsHandle: true)
        {
        }

        public override bool IsInvalid => handle == IntPtr.Zero;

        protected override bool ReleaseHandle()
        {
            Native.kk_document_free(handle);
            return true;
        }
    }
}
