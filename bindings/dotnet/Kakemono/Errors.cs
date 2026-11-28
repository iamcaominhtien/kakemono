using System;

namespace Kakemono
{
    /// <summary>What kind of error a <see cref="KakemonoException"/> is.</summary>
    public enum KakemonoErrorKind
    {
        /// <summary>A wrong argument or option (an unknown mode, a scale out of range).</summary>
        InvalidArgument = 1,

        /// <summary>The document cannot be read, fetched or written so.</summary>
        Document = 2,

        /// <summary>A bug of kakemono.</summary>
        Internal = 3,

        /// <summary>
        /// The call was stopped (a <see cref="System.Threading.CancellationToken"/> cancels it
        /// with an <see cref="OperationCanceledException"/> instead).
        /// </summary>
        Cancelled = 4,

        /// <summary>The call was stopped by a time limit.</summary>
        TimedOut = 5,
    }

    /// <summary>An error of kakemono.</summary>
    public sealed class KakemonoException : Exception
    {
        /// <summary>An error of <paramref name="kind"/> with <paramref name="message"/>.</summary>
        public KakemonoException(string message, KakemonoErrorKind kind)
            : base(message)
        {
            Kind = kind;
        }

        /// <summary>The kind of error.</summary>
        public KakemonoErrorKind Kind { get; }
    }
}
