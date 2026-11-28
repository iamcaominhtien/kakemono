package io.github.iamcaominhtien.kakemono;

import java.lang.ref.Cleaner;
import java.lang.ref.Reference;
import java.time.Duration;

/**
 * A request to stop conversions, shared by the calls given it (with {@link Options#cancel}) and
 * the threads that may stop them.
 *
 * <pre>{@code
 * CancelToken token = new CancelToken();
 * // elsewhere, when the user closes the window or the request is dropped: token.cancel();
 * doc.toMarkdown(new MarkdownOptions().cancel(token).timeout(Duration.ofSeconds(30)));
 * }</pre>
 *
 * <p>A call stopped by {@link #cancel()} throws a {@link KakemonoException} of kind {@link
 * KakemonoException.Kind#CANCELLED} soon after (normally within a page); one past its time limit,
 * of kind {@link KakemonoException.Kind#TIMED_OUT}. The document stays usable.
 */
public final class CancelToken {
    /** Frees the native token once the Java one is unreachable. */
    private record Free(long handle) implements Runnable {
        @Override
        public void run() {
            Native.cancelFree(handle);
        }
    }

    /** The native token: freed by the cleaner only, so never while this is reachable. */
    private final long handle;

    private CancelToken(long handle) {
        this.handle = handle;
        Document.CLEANER.register(this, new Free(handle));
    }

    /** A token with no time limit, not cancelled. */
    public CancelToken() {
        this(Native.cancelNew(0, 0));
    }

    /** A token that times out {@code limit} from now. */
    public static CancelToken withTimeout(Duration limit) {
        return new CancelToken(Native.cancelNew(0, Options.millis(limit)));
    }

    /**
     * A token stopped by this one (by its cancellation or its time limit) and by its own;
     * cancelling it does not stop this one. {@code limit} is its own time limit, from now, or
     * null for none.
     */
    public CancelToken child(Duration limit) {
        try {
            return new CancelToken(Native.cancelNew(handle, limit == null ? 0 : Options.millis(limit)));
        } finally {
            Reference.reachabilityFence(this);
        }
    }

    /** Asks the calls given this token (and its children) to stop. */
    public void cancel() {
        try {
            Native.cancelCancel(handle);
        } finally {
            Reference.reachabilityFence(this);
        }
    }

    /** Whether the calls given this token must stop: cancelled, or past its time limit. */
    public boolean isCancelled() {
        return state() != 0;
    }

    /** Whether the time limit of this token (or of its parent) passed. */
    public boolean isTimedOut() {
        return state() == 5;
    }

    /** 0, 4 (cancelled) or 5 (timed out). */
    private int state() {
        try {
            return Native.cancelState(handle);
        } finally {
            Reference.reachabilityFence(this);
        }
    }

    /** The native token, for a call: the caller keeps this token reachable while it runs. */
    long handle() {
        return handle;
    }
}
